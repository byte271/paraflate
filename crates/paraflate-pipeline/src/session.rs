use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use paraflate_core::{
    ArchiveEntryDescriptor, ArchiveLayout, ArchiveProfile, CompressionMethod,
    DeflateBlockDebugRecord, EntryId, EntryRunStats, ExecutionPhase, ParaflateError,
    ParaflateResult, VerificationMode,
};
use paraflate_deflate::{BlockPlanner, DeflateEngine, DeflateEngineConfig, EntryCompressHints};
use paraflate_dictionary::{GlobalAnalyzer, GlobalModel, SamplePlan};
use paraflate_index::{IndexBuildConfig, PatternIndex};
use paraflate_io::{DirectoryScanner, FileReadPlan, FileReader, ReadOutcome};
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

        // ── Discovery ────────────────────────────────────────────────────────
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

        // ── Sampling + global analysis ────────────────────────────────────────
        // Fast path: for very small archives, skip sampling and use defaults.
        // The analysis overhead dominates for tiny workloads.
        let total_bytes = report.uncompressed_bytes;
        report.phase = ExecutionPhase::Sampling;
        let model = if total_bytes < 64 * 1024 {
            // Skip sampling entirely for small archives.
            GlobalModel::default()
        } else {
            let plan = SamplePlan::from_policy(&profile.execution);
            let samples = Self::collect_samples(&entries, profile, &plan)?;
            report.phase = ExecutionPhase::GlobalAnalysis;
            GlobalAnalyzer::analyze(&entries, &samples, &profile.execution, &plan)
        };

        let graph = TaskGraphBuilder::new()
            .linear_pipeline(&entries.iter().map(|e| e.id).collect::<Vec<_>>());
        let _ = graph;

        // ── File read ─────────────────────────────────────────────────────────
        report.phase = ExecutionPhase::FileRead;
        let reader = FileReader::new(FileReadPlan {
            prefer_mmap_bytes: 4 * 1024 * 1024,
            chunk_bytes: profile.budget.memory.read_chunk_bytes,
        });
        let mut blobs: Vec<Option<Arc<Vec<u8>>>> = vec![None; entries.len()];
        for e in &entries {
            let v = read_file_bytes_fast(&e.path, &reader)?;
            blobs[e.id.0 as usize] = Some(Arc::new(v));
        }

        // ── Predictive planning ───────────────────────────────────────────────
        let pred_plan = build_predictive_archive_plan(
            &entries,
            &blobs,
            &model,
            &profile.execution,
            &profile.predictive,
            profile.compression.method,
        );

        // ── Chunk planning ────────────────────────────────────────────────────
        report.phase = ExecutionPhase::ChunkPlanning;
        struct EntryJob {
            entry: ArchiveEntryDescriptor,
            data: Arc<Vec<u8>>,
            hints: EntryCompressHints,
            chunk_plan: paraflate_core::ChunkPlan,
        }

        let mut entry_methods: Vec<CompressionMethod> = vec![profile.compression.method; entries.len()];
        let mut jobs: Vec<EntryJob> = Vec::with_capacity(entries.len());
        for e in &entries {
            let data = blobs[e.id.0 as usize]
                .as_ref()
                .ok_or_else(|| ParaflateError::InvariantViolated("blob".to_string()))?
                .clone();
            let ep = pred_plan.for_entry(e.id);
            let hints = build_entry_compress_hints(profile, ep, &model);
            entry_methods[e.id.0 as usize] = hints.profile.method;
            let chunk_plan = BlockPlanner::plan_entry_with_data_predictive(
                e.id,
                Some(data.as_slice()),
                e.uncompressed_size,
                &profile.execution,
                &model,
                ep.map(|plan| plan.target_block_bytes),
            );
            jobs.push(EntryJob {
                entry: e.clone(),
                data,
                hints,
                chunk_plan,
            });
        }

        // ── Pattern index ─────────────────────────────────────────────────────
        report.phase = ExecutionPhase::GlobalAnalysis;
        // Skip the index when there is little cross-entry duplication.
        let dup_ratio = model.summary.duplicate_mass as f64
            / model.summary.total_uncompressed.max(1) as f64;
        let use_index = dup_ratio > 0.04 || entries.len() <= 4;
        let index = if use_index {
            let mut idx_pairs: Vec<(u32, &[u8])> = Vec::with_capacity(entries.len());
            for e in &entries {
                let b = blobs[e.id.0 as usize]
                    .as_ref()
                    .ok_or_else(|| ParaflateError::InvariantViolated("blob".to_string()))?;
                idx_pairs.push((e.id.0, b.as_slice()));
            }
            Arc::new(PatternIndex::build(
                &idx_pairs,
                &IndexBuildConfig {
                    stride: model.index_stride.max(1),
                    ..IndexBuildConfig::default()
                },
            ))
        } else {
            Arc::new(PatternIndex::empty())
        };

        // ── Parallel compression ──────────────────────────────────────────────
        // All entries are dispatched to a single persistent worker pool.
        // Workers are spawned once and reused — no per-entry thread overhead.
        report.phase = ExecutionPhase::Compression;

        let engine_cfg = DeflateEngineConfig {
            profile: profile.compression.clone(),
        };
        let engine = Arc::new(DeflateEngine::new(engine_cfg));

        let threads = profile.budget.worker_threads.max(1);

        // One persistent pool for the whole session.
        let shared_pool = Arc::new(WorkerPool::new(WorkerPoolConfig {
            worker_threads: threads,
            queue_depth: threads.saturating_mul(16).max(128),
        }));

        let model_arc = Arc::new(model.clone());
        let index_arc = Arc::clone(&index);
        let profile_arc = Arc::new(profile.clone());
        let debug_arc2 = debug_arc.clone();
        let collect_stats = params.collect_entry_stats;

        type JobResult = (DeflateOutput, Option<EntryRunStats>);

        // Build one closure per entry.
        let mut entry_closures: Vec<
            Box<dyn FnOnce() -> ParaflateResult<JobResult> + Send + 'static>,
        > = Vec::with_capacity(jobs.len());

        for job in jobs {
            let engine_c = Arc::clone(&engine);
            let model_c = Arc::clone(&model_arc);
            let index_c = Arc::clone(&index_arc);
            let profile_c = Arc::clone(&profile_arc);
            let debug_c = debug_arc2.clone();
            let pool_c = Arc::clone(&shared_pool);
            let entry_id = job.entry.id.0;

            entry_closures.push(Box::new(move || {
                let mut out = engine_c.compress_entry(
                    &pool_c,
                    &job.entry,
                    job.data.clone(),
                    &job.chunk_plan,
                    &model_c,
                    Arc::clone(&index_c),
                    Some(&job.hints),
                    &profile_c.execution,
                    debug_c.clone(),
                );

                if let Err(ref err) = out {
                    if should_retry_compression(err) {
                        let fixed_profile = merge_profile_fixed_deflate(&job.hints.profile);
                        let h1 = EntryCompressHints {
                            profile: fixed_profile,
                            lz77_max_chain: None,
                            lz77_nice_match: None,
                            predicted_size_ratio: job.hints.predicted_size_ratio,
                        };
                        out = engine_c.compress_entry(
                            &pool_c,
                            &job.entry,
                            job.data.clone(),
                            &job.chunk_plan,
                            &model_c,
                            Arc::clone(&index_c),
                            Some(&h1),
                            &profile_c.execution,
                            debug_c.clone(),
                        );
                    }
                }
                if let Err(ref err) = out {
                    if should_retry_compression(err) {
                        let stored_profile = merge_profile_stored(&job.hints.profile);
                        let h2 = EntryCompressHints {
                            profile: stored_profile,
                            lz77_max_chain: None,
                            lz77_nice_match: None,
                            predicted_size_ratio: job.hints.predicted_size_ratio,
                        };
                        out = engine_c.compress_entry(
                            &pool_c,
                            &job.entry,
                            job.data,
                            &job.chunk_plan,
                            &model_c,
                            Arc::clone(&index_c),
                            Some(&h2),
                            &profile_c.execution,
                            debug_c,
                        );
                    }
                }

                out.map(|o| {
                    let stats = if collect_stats {
                        let stored_eff =
                            o.compressed.len() as u64 == job.entry.uncompressed_size;
                        Some(EntryRunStats {
                            entry_id,
                            uncompressed_size: job.entry.uncompressed_size,
                            compressed_size: o.compressed.len() as u64,
                            stored_effective: stored_eff,
                        })
                    } else {
                        None
                    };
                    (o, stats)
                })
            }));
        }

        // Dispatch all entry closures to the persistent pool.
        let results = shared_pool.run_parallel(entry_closures)?;

        let mut ordered: Vec<DeflateOutput> = Vec::with_capacity(entries.len());
        let mut entry_stats_acc: Option<Vec<EntryRunStats>> =
            if params.collect_entry_stats { Some(Vec::new()) } else { None };

        for (out, stats) in results {
            if let (Some(ref mut acc), Some(s)) = (&mut entry_stats_acc, stats) {
                acc.push(s);
            }
            ordered.push(out);
        }

        // ── ZIP writing ───────────────────────────────────────────────────────
        report.phase = ExecutionPhase::Encoding;
        report.phase = ExecutionPhase::ZipWriting;
        let file = File::create(&params.output_zip)?;
        let mut zip = ZipWriter::new(file);
        for (e, out) in entries.iter().zip(ordered.iter()) {
            report.compressed_bytes = report
                .compressed_bytes
                .saturating_add(out.compressed.len() as u64);
            let spec = LocalHeaderSpec {
                name: e.logical_name.clone(),
                method: entry_methods[e.id.0 as usize],
                crc32: out.crc32,
                compressed_size: out.compressed.len() as u32,
                uncompressed_size: out.uncompressed_size as u32,
                dos_time: 0,
                dos_date: 0,
            };
            zip.write_local_entry(spec, &out.compressed)?;
        }

        // ── Finalization ──────────────────────────────────────────────────────
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

/// Read a file into memory. Uses mmap for files ≥ 4 MiB to avoid the
/// kernel-copy overhead of `read_to_end`.
fn read_file_bytes_fast(path: &PathBuf, reader: &FileReader) -> ParaflateResult<Vec<u8>> {
    match reader.read_path_mmap(path)? {
        ReadOutcome::Mmap(m) => Ok(m.to_vec()),
        ReadOutcome::Inline(v) => Ok(v),
        ReadOutcome::Buffer(b) => Ok(b.as_slice().to_vec()),
    }
}
