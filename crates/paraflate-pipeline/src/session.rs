use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use paraflate_core::{
    ArchiveEntryDescriptor, ArchiveLayout, ArchiveProfile, DeflateBlockDebugRecord, EntryId,
    EntryRunStats, ExecutionPhase, ParaflateError, ParaflateResult, VerificationMode,
};
use paraflate_deflate::{BlockPlanner, DeflateEngine, DeflateEngineConfig, EntryCompressHints};
use paraflate_dictionary::{GlobalAnalyzer, SamplePlan};
use paraflate_index::{IndexBuildConfig, PatternIndex};
use paraflate_io::{DirectoryScanner, FileReadPlan, FileReader};
use paraflate_scheduler::{TaskGraphBuilder, WorkerPool, WorkerPoolConfig};
use paraflate_zip::{LocalHeaderSpec, ZipWriter};

use crate::fallback_policy::{
    merge_profile_fixed_deflate, merge_profile_stored, should_retry_compression,
};
use crate::predictive_plan::{build_entry_compress_hints, build_predictive_archive_plan};
use crate::report::RunReport;
use crate::verification::verify_zip_path;
use paraflate_deflate::DeflateOutput;

#[derive(Clone, Debug)]
pub struct CreateArchiveParams {
    pub input_root: PathBuf,
    pub output_zip: PathBuf,
    pub profile: ArchiveProfile,
    pub debug_blocks: bool,
    pub collect_entry_stats: bool,
}

impl CreateArchiveParams {
    pub fn new(input_root: PathBuf, output_zip: PathBuf, profile: ArchiveProfile) -> Self {
        Self {
            input_root,
            output_zip,
            profile,
            debug_blocks: false,
            collect_entry_stats: false,
        }
    }

    pub fn with_debug(mut self, v: bool) -> Self {
        self.debug_blocks = v;
        self
    }

    pub fn with_entry_stats(mut self, v: bool) -> Self {
        self.collect_entry_stats = v;
        self
    }
}

pub struct ArchiveSession;

impl ArchiveSession {
    pub fn new() -> Self {
        Self
    }

    pub fn create_archive(&self, params: CreateArchiveParams) -> ParaflateResult<RunReport> {
        let t0 = Instant::now();
        let profile = &params.profile;
        let mut report = RunReport {
            phase: ExecutionPhase::Discovery,
            entries: 0,
            uncompressed_bytes: 0,
            compressed_bytes: 0,
            worker_threads: profile.budget.worker_threads,
            zip: None,
            verification: None,
            elapsed_ms: 0,
            predictive_mode: profile.predictive.mode,
            verification_mode: profile.predictive.verification,
            debug_blocks: None,
            entry_stats: None,
        };
        let debug_arc = if params.debug_blocks {
            Some(Arc::new(Mutex::new(Vec::<DeflateBlockDebugRecord>::new())))
        } else {
            None
        };
        let mut entry_stats_acc = if params.collect_entry_stats {
            Some(Vec::<EntryRunStats>::new())
        } else {
            None
        };
        let scanner = DirectoryScanner::new(&params.input_root);
        let scan = scanner.scan()?;
        if scan.entries.is_empty() {
            return Err(ParaflateError::EmptyArchive);
        }
        report.phase = ExecutionPhase::MetadataScan;
        report.entries = scan.entries.len() as u64;
        for e in &scan.entries {
            report.uncompressed_bytes = report
                .uncompressed_bytes
                .saturating_add(e.uncompressed_size);
        }
        let mut entries = scan.entries;
        Self::apply_layout(&mut entries, profile);
        report.phase = ExecutionPhase::Sampling;
        let plan = SamplePlan::from_policy(&profile.execution);
        let samples = Self::collect_samples(&entries, profile, &plan)?;
        report.phase = ExecutionPhase::GlobalAnalysis;
        let model = GlobalAnalyzer::analyze(&entries, &samples, &profile.execution, &plan);
        let graph = TaskGraphBuilder::new()
            .linear_pipeline(&entries.iter().map(|e| e.id).collect::<Vec<_>>());
        let _ = graph;
        report.phase = ExecutionPhase::BlockScheduling;
        let pool_cfg = WorkerPoolConfig {
            worker_threads: profile.budget.worker_threads,
            queue_depth: profile.budget.max_pending_tasks,
        };
        let pool = WorkerPool::new(pool_cfg);
        report.phase = ExecutionPhase::FileRead;
        let mut blobs: Vec<Option<Arc<Vec<u8>>>> = vec![None; entries.len()];
        for e in &entries {
            let v = read_file_bytes(&e.path)?;
            blobs[e.id.0 as usize] = Some(Arc::new(v));
        }
        let pred_plan = build_predictive_archive_plan(
            &entries,
            &blobs,
            &model,
            &profile.execution,
            &profile.predictive,
            profile.compression.method,
        );
        report.phase = ExecutionPhase::ChunkPlanning;
        let mut plans = BTreeMap::new();
        for e in &entries {
            let data_ref = blobs[e.id.0 as usize]
                .as_ref()
                .ok_or_else(|| ParaflateError::InvariantViolated("blob".to_string()))?;
            let tb = pred_plan.for_entry(e.id).map(|p| p.target_block_bytes);
            let p = BlockPlanner::plan_entry_with_data_predictive(
                e.id,
                Some(data_ref.as_slice()),
                e.uncompressed_size,
                &profile.execution,
                &model,
                tb,
            );
            plans.insert(e.id.0, p);
        }
        report.phase = ExecutionPhase::GlobalAnalysis;
        let mut idx_pairs: Vec<(u32, &[u8])> = Vec::new();
        for e in &entries {
            let b = blobs[e.id.0 as usize]
                .as_ref()
                .ok_or_else(|| ParaflateError::InvariantViolated("blob".to_string()))?;
            idx_pairs.push((e.id.0, b.as_slice()));
        }
        let index = Arc::new(PatternIndex::build(
            &idx_pairs,
            &IndexBuildConfig {
                stride: model.index_stride.max(1),
                ..IndexBuildConfig::default()
            },
        ));
        report.phase = ExecutionPhase::Compression;
        let engine_cfg = DeflateEngineConfig {
            profile: profile.compression.clone(),
        };
        let engine = DeflateEngine::new(engine_cfg);
        let mut ordered: Vec<(u32, DeflateOutput)> = Vec::new();
        for e in &entries {
            let data = blobs[e.id.0 as usize]
                .as_ref()
                .ok_or_else(|| ParaflateError::InvariantViolated("blob".to_string()))?
                .clone();
            let chunk_plan = plans
                .get(&e.id.0)
                .ok_or_else(|| ParaflateError::EntryNotFound(e.logical_name.clone()))?;
            let ep = pred_plan.for_entry(e.id);
            let hints = build_entry_compress_hints(profile, ep, &model);
            let mut out = engine.compress_entry(
                &pool,
                e,
                data.clone(),
                chunk_plan,
                &model,
                Arc::clone(&index),
                Some(&hints),
                &profile.execution,
                debug_arc.clone(),
            );
            if let Err(ref err) = out {
                if should_retry_compression(err) {
                    let fixed_profile = merge_profile_fixed_deflate(&hints.profile);
                    let h1 = EntryCompressHints {
                        profile: fixed_profile,
                        lz77_max_chain: None,
                        lz77_nice_match: None,
                        predicted_size_ratio: hints.predicted_size_ratio,
                    };
                    out = engine.compress_entry(
                        &pool,
                        e,
                        data.clone(),
                        chunk_plan,
                        &model,
                        Arc::clone(&index),
                        Some(&h1),
                        &profile.execution,
                        debug_arc.clone(),
                    );
                }
            }
            if let Err(ref err) = out {
                if should_retry_compression(err) {
                    let stored_profile = merge_profile_stored(&hints.profile);
                    let h2 = EntryCompressHints {
                        profile: stored_profile,
                        lz77_max_chain: None,
                        lz77_nice_match: None,
                        predicted_size_ratio: hints.predicted_size_ratio,
                    };
                    out = engine.compress_entry(
                        &pool,
                        e,
                        data,
                        chunk_plan,
                        &model,
                        Arc::clone(&index),
                        Some(&h2),
                        &profile.execution,
                        debug_arc.clone(),
                    );
                }
            }
            let out = out?;
            if let Some(ref mut es) = entry_stats_acc {
                let stored_eff = out.compressed.len() as u64 == e.uncompressed_size;
                es.push(EntryRunStats {
                    entry_id: e.id.0,
                    uncompressed_size: e.uncompressed_size,
                    compressed_size: out.compressed.len() as u64,
                    stored_effective: stored_eff,
                });
            }
            ordered.push((e.id.0, out));
        }
        ordered.sort_by_key(|(k, _)| *k);
        report.phase = ExecutionPhase::Encoding;
        report.phase = ExecutionPhase::ZipWriting;
        let file = File::create(&params.output_zip)?;
        let mut zip = ZipWriter::new(file);
        for e in &entries {
            let out = ordered
                .iter()
                .find(|(id, _)| *id == e.id.0)
                .map(|(_, v)| v)
                .ok_or_else(|| ParaflateError::EntryNotFound(e.logical_name.clone()))?;
            report.compressed_bytes = report
                .compressed_bytes
                .saturating_add(out.compressed.len() as u64);
            let ep = pred_plan.for_entry(e.id);
            let hints = build_entry_compress_hints(profile, ep, &model);
            let method = hints.profile.method;
            let spec = LocalHeaderSpec {
                name: e.logical_name.clone(),
                method,
                crc32: out.crc32,
                compressed_size: out.compressed.len() as u32,
                uncompressed_size: out.uncompressed_size as u32,
                dos_time: 0,
                dos_date: 0,
            };
            zip.write_local_entry(spec, &out.compressed)?;
        }
        report.phase = ExecutionPhase::Finalization;
        let (sink, summary) = zip.finalize()?;
        sink.sync_all()?;
        report.zip = Some(summary);
        if profile.predictive.verification != VerificationMode::Off {
            let vr = verify_zip_path(&params.output_zip, profile.predictive.verification)?;
            report.verification = Some(vr);
        }
        report.elapsed_ms = t0.elapsed().as_millis();
        if let Some(a) = debug_arc {
            if let Ok(g) = a.lock() {
                report.debug_blocks = Some(g.clone());
            }
        }
        report.entry_stats = entry_stats_acc;
        Ok(report)
    }

    pub(crate) fn apply_layout(
        entries: &mut Vec<ArchiveEntryDescriptor>,
        profile: &ArchiveProfile,
    ) {
        match profile.layout {
            ArchiveLayout::DeterministicLexical => {
                entries.sort_by(|a, b| a.logical_name.cmp(&b.logical_name));
            }
            ArchiveLayout::SizeDescending => {
                entries.sort_by(|a, b| b.uncompressed_size.cmp(&a.uncompressed_size));
            }
            ArchiveLayout::GlobalScoreDescending => {
                entries.sort_by(|a, b| b.uncompressed_size.cmp(&a.uncompressed_size));
            }
        }
        for (idx, e) in entries.iter_mut().enumerate() {
            e.id = EntryId(idx as u32);
        }
    }

    fn collect_samples(
        entries: &[ArchiveEntryDescriptor],
        profile: &ArchiveProfile,
        plan: &SamplePlan,
    ) -> ParaflateResult<Vec<(usize, Vec<u8>)>> {
        let reader = FileReader::new(FileReadPlan {
            prefer_mmap_bytes: u64::MAX,
            chunk_bytes: profile.budget.memory.read_chunk_bytes,
        });
        let mut out = Vec::new();
        for (idx, e) in entries.iter().enumerate() {
            let window = plan.window_for_entry(e.uncompressed_size);
            if window == 0 {
                out.push((idx, Vec::new()));
                continue;
            }
            let mut file = File::open(&e.path)?;
            let mut buf = vec![0u8; window];
            let n = file.read(&mut buf)?;
            buf.truncate(n);
            out.push((idx, buf));
        }
        let _ = reader;
        Ok(out)
    }
}

fn read_file_bytes(path: &PathBuf) -> ParaflateResult<Vec<u8>> {
    let mut f = File::open(path)?;
    let mut v = Vec::new();
    f.read_to_end(&mut v)?;
    Ok(v)
}
