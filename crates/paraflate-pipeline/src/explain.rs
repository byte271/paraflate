use paraflate_core::{
    ArchiveProfile, DeflateBlockDebugRecord, DeflateStrategy, EntryRunStats, ExplainArchive,
    ExplainBlock, ExplainFile, ExplainMatches, ExplainReport, PredictedDeflatePath,
};

use crate::intel::{ArchiveIntelReport, FileIntelRow};
use crate::report::RunReport;

pub fn build_explain_report(
    intel: &ArchiveIntelReport,
    run: Option<&RunReport>,
    profile: &ArchiveProfile,
    no_run: bool,
) -> ExplainReport {
    let weighted_pred: f64 = if intel.total_raw_bytes == 0 {
        0.0
    } else {
        intel
            .files
            .iter()
            .map(|f| f.est_compressed_ratio * f.bytes as f64)
            .sum::<f64>()
            / intel.total_raw_bytes as f64
    };
    let actual_ratio = if no_run {
        0.0
    } else {
        run.map(|r| {
            if r.uncompressed_bytes == 0 {
                0.0
            } else {
                r.compressed_bytes as f64 / r.uncompressed_bytes as f64
            }
        })
        .unwrap_or(0.0)
    };
    let archive = ExplainArchive {
        entropy: intel.global_entropy_bits,
        strategy: strategy_label(profile.compression.strategy),
        global_huffman: profile.compression.global_huffman,
        predicted_ratio: weighted_pred,
        actual_ratio,
    };
    let dbg: &[DeflateBlockDebugRecord] =
        run.and_then(|r| r.debug_blocks.as_deref()).unwrap_or(&[]);
    let mut files = Vec::with_capacity(intel.files.len());
    for (i, f) in intel.files.iter().enumerate() {
        let eid = i as u32;
        let stat: Option<&EntryRunStats> = run
            .and_then(|r| r.entry_stats.as_ref())
            .and_then(|s| s.iter().find(|e| e.entry_id == eid));
        let predicted_ratio = f.est_compressed_ratio;
        let actual_ratio = if no_run {
            0.0
        } else {
            stat.map(|s| {
                if s.uncompressed_size == 0 {
                    0.0
                } else {
                    s.compressed_size as f64 / s.uncompressed_size as f64
                }
            })
            .unwrap_or(0.0)
        };
        let actual_label = if no_run {
            String::new()
        } else {
            stat.map(|s| {
                if s.stored_effective {
                    "stored".to_string()
                } else {
                    "deflate".to_string()
                }
            })
            .unwrap_or_else(String::new)
        };
        let mut rows: Vec<&DeflateBlockDebugRecord> =
            dbg.iter().filter(|b| b.entry_id == eid).collect();
        rows.sort_by_key(|b| b.block_id);
        let blocks: Vec<ExplainBlock> = rows
            .into_iter()
            .map(|r| {
                let dom = match_dominance(r.lz_matches_window, r.lz_matches_index);
                let (fb, reason) = block_fallback(r);
                ExplainBlock {
                    id: r.block_id,
                    offset: r.offset,
                    length: r.raw_span_bytes,
                    predicted: format!("{:.6}", r.predicted_ratio),
                    actual: format!("{:.6}", r.actual_ratio),
                    predicted_ratio: r.predicted_ratio,
                    actual_ratio: r.actual_ratio,
                    matches: ExplainMatches {
                        window: r.lz_matches_window,
                        index: r.lz_matches_index,
                    },
                    match_dominance: dom.to_string(),
                    fallback: fb,
                    fallback_reason: reason,
                    lz77_tokens: r.token_count,
                    compressed_bytes: r.compressed_bytes,
                }
            })
            .collect();
        files.push(ExplainFile {
            name: f.path.clone(),
            size: f.bytes,
            entropy: f.entropy_bits,
            predicted: file_predicted_str(f),
            actual: actual_label,
            predicted_ratio,
            actual_ratio,
            blocks,
        });
    }
    ExplainReport { archive, files }
}

fn strategy_label(s: DeflateStrategy) -> String {
    match s {
        DeflateStrategy::Default => "Default".to_string(),
        DeflateStrategy::Filtered => "Filtered".to_string(),
        DeflateStrategy::HuffmanOnly => "HuffmanOnly".to_string(),
        DeflateStrategy::Rle => "Rle".to_string(),
        DeflateStrategy::Fixed => "Fixed".to_string(),
    }
}

fn file_predicted_str(f: &FileIntelRow) -> String {
    if f.predicted_stored {
        return "stored".to_string();
    }
    let tail = match f.predicted_deflate_path {
        PredictedDeflatePath::Auto => "Auto",
        PredictedDeflatePath::Dynamic => "Dynamic",
        PredictedDeflatePath::Fixed => "Fixed",
    };
    format!("deflate:{tail}")
}

fn match_dominance(win: u32, idx: u32) -> &'static str {
    let wf = win as f64;
    let ix = idx as f64;
    let m = wf.max(ix).max(1.0);
    if (ix - wf).abs() / m <= 0.12 {
        "balanced"
    } else if idx > win {
        "index-dominant"
    } else {
        "window-dominant"
    }
}

fn block_fallback(r: &DeflateBlockDebugRecord) -> (bool, String) {
    if r.fallback_code != 0 {
        return (true, format!("code_{}", r.fallback_code));
    }
    if r.actual_ratio > r.predicted_ratio * 1.15 && r.raw_span_bytes >= 4096 {
        return (true, "dynamic_not_beneficial".to_string());
    }
    (false, String::new())
}
