use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crc32fast::Hasher;
use paraflate_core::{
    ArchiveEntryDescriptor, BlockId, ChunkPlan, CompressionMethod, CompressionProfile,
    DeflateBlockDebugRecord, ExecutionPolicy, ParaflateError, ParaflateResult,
};
use paraflate_dictionary::GlobalModel;
use paraflate_index::{IndexBuildConfig, PatternIndex};
use paraflate_lz77::{compress_block, Lz77BlockParams, Lz77Config, Lz77Token};
use paraflate_scheduler::{CompressionWork, WorkerPool};

use crate::plan::BlockPlanner;
use crate::stream::{encode_deflate_blocks, encode_one_deflate_block, DeflateEncodeOptions};

#[derive(Clone, Debug)]
pub struct DeflateEngineConfig {
    pub profile: CompressionProfile,
}

#[derive(Clone, Debug)]
pub struct EntryCompressHints {
    pub profile: CompressionProfile,
    pub lz77_max_chain: Option<u32>,
    pub lz77_nice_match: Option<u32>,
    pub predicted_size_ratio: f64,
}

impl Default for EntryCompressHints {
    fn default() -> Self {
        Self {
            profile: CompressionProfile::default(),
            lz77_max_chain: None,
            lz77_nice_match: None,
            predicted_size_ratio: 0.72,
        }
    }
}

pub struct DeflateOutput {
    pub compressed: Vec<u8>,
    pub crc32: u32,
    pub uncompressed_size: u64,
}

#[derive(Clone)]
pub struct Lz77JobOutput {
    pub block: BlockId,
    pub tokens: Vec<Lz77Token>,
    pub window_matches: u32,
    pub index_matches: u32,
}

#[derive(Clone)]
pub struct DeflateEngine {
    cfg: DeflateEngineConfig,
}

impl DeflateEngine {
    pub fn new(cfg: DeflateEngineConfig) -> Self {
        Self { cfg }
    }

    pub fn compress_entry(
        &self,
        pool: &WorkerPool,
        entry: &ArchiveEntryDescriptor,
        data: Arc<Vec<u8>>,
        plan: &ChunkPlan,
        model: &GlobalModel,
        index: Arc<PatternIndex>,
        hints: Option<&EntryCompressHints>,
        policy: &ExecutionPolicy,
        debug_trace: Option<Arc<Mutex<Vec<DeflateBlockDebugRecord>>>>,
    ) -> ParaflateResult<DeflateOutput> {
        if data.len() as u64 != entry.uncompressed_size {
            return Err(ParaflateError::InvariantViolated(
                "entry size mismatch".to_string(),
            ));
        }
        let default_hints = EntryCompressHints {
            profile: self.cfg.profile.clone(),
            ..EntryCompressHints::default()
        };
        let hints = hints.unwrap_or(&default_hints);
        let profile = &hints.profile;
        match profile.method {
            CompressionMethod::Stored => {
                let mut hasher = Hasher::new();
                hasher.update(data.as_slice());
                Ok(DeflateOutput {
                    compressed: data.to_vec(),
                    crc32: hasher.finalize(),
                    uncompressed_size: entry.uncompressed_size,
                })
            }
            CompressionMethod::Deflate => {
                let adaptive = policy.adaptive_block_feedback
                    && plan.spans.len() > 1
                    && !profile.global_huffman;
                if adaptive {
                    self.compress_deflate_adaptive(
                        pool,
                        entry,
                        data,
                        plan,
                        model,
                        index,
                        hints,
                        debug_trace,
                    )
                } else {
                    self.compress_deflate_parallel(
                        pool,
                        entry,
                        data,
                        plan,
                        model,
                        index,
                        hints,
                        debug_trace,
                    )
                }
            }
        }
    }

    fn compress_deflate_parallel(
        &self,
        pool: &WorkerPool,
        entry: &ArchiveEntryDescriptor,
        data: Arc<Vec<u8>>,
        plan: &ChunkPlan,
        model: &GlobalModel,
        index: Arc<PatternIndex>,
        hints: &EntryCompressHints,
        debug_trace: Option<Arc<Mutex<Vec<DeflateBlockDebugRecord>>>>,
    ) -> ParaflateResult<DeflateOutput> {
        let profile = &hints.profile;
        let mut jobs = Vec::new();
        for span in &plan.spans {
            let job_key = ((entry.id.0 as u64) << 32) ^ (span.block.0 as u32 as u64);
            jobs.push(CompressionWork {
                job_key,
                entry: entry.id,
                data: Arc::clone(&data),
                span: *span,
            });
        }
        let max_chain = hints.lz77_max_chain.unwrap_or(model.lz77_max_chain) as usize;
        let nice_match = hints.lz77_nice_match.unwrap_or(model.lz77_nice_match) as usize;
        let lz_cfg = Lz77Config {
            max_chain,
            nice_match,
        };
        let index_c = Arc::clone(&index);
        let entry_id = entry.id.0;
        let outcome = pool.execute(jobs, move |w| {
            let span = w.span;
            let off = span.offset as usize;
            let end = off.saturating_add(span.len as usize);
            if end > w.data.len() {
                return Err(ParaflateError::InvariantViolated("span bounds".to_string()));
            }
            let overlap = off.min(32768);
            let start = off - overlap;
            let buf = w.data[start..end].to_vec();
            let emit_start = off - start;
            let emit_end = buf.len();
            let params = Lz77BlockParams {
                entry: entry_id,
                entry_rel_base: start as u64,
                emit_start,
                emit_end,
            };
            let lz = compress_block(&buf, &params, &lz_cfg, Some(index_c.as_ref()));
            Ok(Lz77JobOutput {
                block: span.block,
                tokens: lz.tokens,
                window_matches: lz.matches_from_window,
                index_matches: lz.matches_from_index,
            })
        })?;
        let mut parts: Vec<Lz77JobOutput> = outcome.results.into_values().collect();
        parts.sort_by_key(|p| p.block.0);
        let blocks: Vec<Vec<Lz77Token>> = if parts.is_empty() {
            vec![Vec::new()]
        } else {
            parts.iter().map(|p| p.tokens.clone()).collect()
        };
        let opts = DeflateEncodeOptions {
            global_huffman: profile.global_huffman,
        };
        let compressed = encode_deflate_blocks(&blocks, profile.strategy, &opts)?;
        if let Some(dt) = debug_trace.as_ref() {
            let n = parts.len();
            let total_compressed = compressed.len() as u64;
            let total_raw: u64 = parts
                .iter()
                .filter_map(|p| {
                    plan.spans
                        .iter()
                        .find(|s| s.block.0 == p.block.0)
                        .map(|s| s.len)
                })
                .sum();
            for (i, p) in parts.iter().enumerate() {
                let span = plan.spans.iter().find(|s| s.block.0 == p.block.0);
                let raw = span.map(|s| s.len).unwrap_or(0);
                let off = span.map(|s| s.offset).unwrap_or(0);
                let cb = if profile.global_huffman && n > 1 && total_raw > 0 {
                    total_compressed.saturating_mul(raw) / total_raw
                } else {
                    let seg = encode_one_deflate_block(
                        &p.tokens,
                        profile.strategy,
                        &opts,
                        i + 1 == n.max(1),
                    )?;
                    seg.len() as u64
                };
                let act = if raw > 0 { cb as f64 / raw as f64 } else { 0.0 };
                if let Ok(mut g) = dt.lock() {
                    g.push(DeflateBlockDebugRecord {
                        entry_id: entry.id.0,
                        block_id: p.block.0,
                        offset: off,
                        raw_span_bytes: raw,
                        compressed_bytes: cb,
                        token_count: p.tokens.len(),
                        predicted_ratio: hints.predicted_size_ratio,
                        actual_ratio: act,
                        lz_matches_window: p.window_matches,
                        lz_matches_index: p.index_matches,
                        fallback_code: 0,
                    });
                }
            }
        }
        let mut hasher = Hasher::new();
        hasher.update(data.as_slice());
        Ok(DeflateOutput {
            compressed,
            crc32: hasher.finalize(),
            uncompressed_size: entry.uncompressed_size,
        })
    }

    fn compress_deflate_adaptive(
        &self,
        pool: &WorkerPool,
        entry: &ArchiveEntryDescriptor,
        data: Arc<Vec<u8>>,
        plan: &ChunkPlan,
        model: &GlobalModel,
        index: Arc<PatternIndex>,
        hints: &EntryCompressHints,
        debug_trace: Option<Arc<Mutex<Vec<DeflateBlockDebugRecord>>>>,
    ) -> ParaflateResult<DeflateOutput> {
        let _ = pool;
        let profile = &hints.profile;
        let mut max_chain = hints.lz77_max_chain.unwrap_or(model.lz77_max_chain) as usize;
        let mut nice_match = hints.lz77_nice_match.unwrap_or(model.lz77_nice_match) as usize;
        let mut blocks: Vec<Vec<Lz77Token>> = Vec::new();
        let mut lz_meta: Vec<(u64, u32, u32, usize)> = Vec::new();
        let n = plan.spans.len();
        for span in plan.spans.iter() {
            let off = span.offset as usize;
            let end = off.saturating_add(span.len as usize);
            if end > data.len() {
                return Err(ParaflateError::InvariantViolated("span bounds".to_string()));
            }
            let overlap = off.min(32768);
            let start = off - overlap;
            let buf = &data[start..end];
            let emit_start = off - start;
            let emit_end = buf.len();
            let params = Lz77BlockParams {
                entry: entry.id.0,
                entry_rel_base: start as u64,
                emit_start,
                emit_end,
            };
            let lz_cfg = Lz77Config {
                max_chain,
                nice_match,
            };
            let lz = compress_block(buf, &params, &lz_cfg, Some(index.as_ref()));
            let raw = span.len;
            let pred = hints.predicted_size_ratio;
            let act_proxy = lz.tokens.len() as f64 / raw.max(1) as f64;
            if act_proxy > pred * 1.12 && raw >= 4096 {
                max_chain = (max_chain as f64 * 0.88).round() as usize;
                nice_match = (nice_match as f64 * 0.92).round() as usize;
                max_chain = max_chain.max(48);
                nice_match = nice_match.max(32);
            } else if act_proxy < pred * 0.92 && raw >= 4096 {
                max_chain = (max_chain as f64 * 1.06).round() as usize;
                nice_match = (nice_match as f64 * 1.04).round() as usize;
                max_chain = max_chain.min(1024);
                nice_match = nice_match.min(258);
            }
            lz_meta.push((
                span.block.0,
                lz.matches_from_window,
                lz.matches_from_index,
                lz.tokens.len(),
            ));
            blocks.push(lz.tokens);
        }
        let opts = DeflateEncodeOptions {
            global_huffman: false,
        };
        let compressed = encode_deflate_blocks(&blocks, profile.strategy, &opts)?;
        if let Some(dt) = debug_trace.as_ref() {
            let np = blocks.len();
            let total_compressed = compressed.len() as u64;
            let total_raw: u64 = plan.spans.iter().map(|s| s.len).sum();
            for (i, span) in plan.spans.iter().enumerate() {
                let raw = span.len;
                let pred = hints.predicted_size_ratio;
                let seg = encode_one_deflate_block(
                    &blocks[i],
                    profile.strategy,
                    &opts,
                    i + 1 == n.max(1),
                )?;
                let cb = if profile.global_huffman && np > 1 && total_raw > 0 {
                    total_compressed.saturating_mul(raw) / total_raw
                } else {
                    seg.len() as u64
                };
                let act = if raw > 0 { cb as f64 / raw as f64 } else { 0.0 };
                let (bid, win, idx, tokc) = lz_meta[i];
                if let Ok(mut g) = dt.lock() {
                    g.push(DeflateBlockDebugRecord {
                        entry_id: entry.id.0,
                        block_id: bid,
                        offset: span.offset,
                        raw_span_bytes: raw,
                        compressed_bytes: cb,
                        token_count: tokc,
                        predicted_ratio: pred,
                        actual_ratio: act,
                        lz_matches_window: win,
                        lz_matches_index: idx,
                        fallback_code: 0,
                    });
                }
            }
        }
        let mut hasher = Hasher::new();
        hasher.update(data.as_slice());
        Ok(DeflateOutput {
            compressed,
            crc32: hasher.finalize(),
            uncompressed_size: entry.uncompressed_size,
        })
    }

    pub fn compress_bytes_raw(&self, data: &[u8]) -> ParaflateResult<DeflateOutput> {
        let entry = ArchiveEntryDescriptor {
            id: paraflate_core::EntryId(0),
            path: PathBuf::new(),
            logical_name: String::new(),
            uncompressed_size: data.len() as u64,
            is_directory: false,
        };
        let plan = BlockPlanner::plan_entry_with_data(
            entry.id,
            Some(data),
            entry.uncompressed_size,
            &ExecutionPolicy::default(),
            &GlobalModel::default(),
        );
        let arc = Arc::new(data.to_vec());
        let index = Arc::new(PatternIndex::build(
            &[(0u32, data)],
            &IndexBuildConfig::default(),
        ));
        let pool = WorkerPool::new(paraflate_scheduler::WorkerPoolConfig {
            worker_threads: 1,
            queue_depth: 16,
        });
        self.compress_entry(
            &pool,
            &entry,
            arc,
            &plan,
            &GlobalModel::default(),
            index,
            None,
            &ExecutionPolicy::default(),
            None,
        )
    }
}
