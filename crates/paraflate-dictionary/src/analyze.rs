use std::collections::HashMap;

use paraflate_core::{
    ArchiveEntryDescriptor, ByteClassHistogram, ExecutionPolicy, GlobalStatisticsSummary,
    PatternDigest,
};

use crate::SamplePlan;

#[derive(Clone, Debug)]
pub struct GlobalModel {
    pub summary: GlobalStatisticsSummary,
    pub suggested_block_bytes: usize,
    pub suggested_order: Vec<u32>,
    pub lz77_max_chain: u32,
    pub lz77_nice_match: u32,
    pub index_stride: usize,
}

impl Default for GlobalModel {
    fn default() -> Self {
        Self {
            summary: GlobalStatisticsSummary::default(),
            suggested_block_bytes: 256 * 1024,
            suggested_order: Vec::new(),
            lz77_max_chain: 128,
            lz77_nice_match: 128,
            index_stride: 1,
        }
    }
}

pub struct GlobalAnalyzer;

impl GlobalAnalyzer {
    pub fn analyze(
        entries: &[ArchiveEntryDescriptor],
        samples: &[(usize, Vec<u8>)],
        policy: &ExecutionPolicy,
        plan: &SamplePlan,
    ) -> GlobalModel {
        let mut hist = ByteClassHistogram::default();
        let mut total = 0u64;
        let mut text_mass = 0u64;
        let mut dup_mass = 0u64;
        let mut rolling: HashMap<u64, u32> = HashMap::new();
        for entry in entries {
            total = total.saturating_add(entry.uncompressed_size);
        }
        for (_, bytes) in samples {
            Self::digest_bytes(
                bytes,
                &mut hist,
                &mut rolling,
                &mut text_mass,
                &mut dup_mass,
            );
        }
        let mut top: Vec<(PatternDigest, u32)> = rolling
            .into_iter()
            .map(|(h, c)| (PatternDigest(h), c))
            .collect();
        top.sort_by(|a, b| b.1.cmp(&a.1));
        top.truncate(32);
        let entry_count = entries.len() as u64;
        let mean = if entry_count == 0 {
            0
        } else {
            total / entry_count
        };
        let summary = GlobalStatisticsSummary {
            total_uncompressed: total,
            entry_count,
            histogram: hist,
            top_patterns: top,
            mean_entry_bytes: mean,
            duplicate_mass: dup_mass,
            text_mass: text_mass,
        };
        let suggested_block_bytes = Self::block_size(&summary, policy, plan, entries.len().max(1));
        let suggested_order = Self::order(entries, &summary);
        let lz77_max_chain = if summary.duplicate_mass > summary.text_mass {
            128u32
        } else {
            64u32
        };
        let lz77_nice_match = if summary.mean_entry_bytes > (1024 * 1024) {
            128u32
        } else {
            64u32
        };
        let index_stride = if summary.duplicate_mass > summary.text_mass {
            1usize
        } else {
            2usize
        };
        GlobalModel {
            summary,
            suggested_block_bytes,
            suggested_order,
            lz77_max_chain,
            lz77_nice_match,
            index_stride,
        }
    }

    fn digest_bytes(
        bytes: &[u8],
        hist: &mut ByteClassHistogram,
        rolling: &mut HashMap<u64, u32>,
        text_mass: &mut u64,
        dup_mass: &mut u64,
    ) {
        let mut prev: Option<u64> = None;
        for (idx, &b) in bytes.iter().enumerate() {
            if b == 0 {
                hist.zero = hist.zero.saturating_add(1);
            } else if b.is_ascii_graphic() || b == b'\n' || b == b'\r' || b == b'\t' {
                hist.ascii_text = hist.ascii_text.saturating_add(1);
                *text_mass = text_mass.saturating_add(1);
            } else {
                hist.high_entropy = hist.high_entropy.saturating_add(1);
            }
            let h = Self::hash64(b, idx as u64);
            let e = rolling.entry(h).or_insert(0);
            *e = e.saturating_add(1);
            if prev == Some(h) {
                hist.duplicate_window_hits = hist.duplicate_window_hits.saturating_add(1);
                *dup_mass = dup_mass.saturating_add(1);
            }
            prev = Some(h.rotate_left(7) ^ h);
        }
    }

    fn hash64(b: u8, pos: u64) -> u64 {
        let mut x = pos.wrapping_mul(0x9E37_79B1_85EB_CA87)
            ^ (b as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
        x ^= x >> 33;
        x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
        x ^= x >> 33;
        x
    }

    fn block_size(
        summary: &GlobalStatisticsSummary,
        policy: &ExecutionPolicy,
        plan: &SamplePlan,
        entry_count: usize,
    ) -> usize {
        let base = policy.base_block_bytes;
        let max_b = policy.max_block_bytes;
        let min_b = policy.min_block_bytes;
        let text_bias = if summary.text_mass > summary.duplicate_mass {
            base.saturating_mul(2).min(max_b)
        } else {
            base
        };
        let small_files_bias = if summary.mean_entry_bytes < (256 * 1024) {
            min_b.max(base / 4)
        } else {
            text_bias
        };
        let mem_bias = plan.max_bytes_per_entry.max(4096).saturating_mul(4);
        let merged = small_files_bias.min(mem_bias).clamp(min_b, max_b);
        let fanout = entry_count.max(1);
        let spread = max_b / fanout.max(1);
        merged.min(spread.max(min_b)).clamp(min_b, max_b)
    }

    fn order(entries: &[ArchiveEntryDescriptor], summary: &GlobalStatisticsSummary) -> Vec<u32> {
        let mut idx: Vec<u32> = entries.iter().map(|e| e.id.0).collect();
        if summary.duplicate_mass > summary.text_mass {
            idx.sort_by(|a, b| {
                let ea = entries
                    .iter()
                    .find(|e| e.id.0 == *a)
                    .map(|e| e.uncompressed_size);
                let eb = entries
                    .iter()
                    .find(|e| e.id.0 == *b)
                    .map(|e| e.uncompressed_size);
                match (ea, eb) {
                    (Some(x), Some(y)) => y.cmp(&x),
                    _ => std::cmp::Ordering::Equal,
                }
            });
        } else {
            idx.sort_by(|a, b| {
                let na = entries
                    .iter()
                    .find(|e| e.id.0 == *a)
                    .map(|e| e.logical_name.as_str())
                    .unwrap_or("");
                let nb = entries
                    .iter()
                    .find(|e| e.id.0 == *b)
                    .map(|e| e.logical_name.as_str())
                    .unwrap_or("");
                na.cmp(nb)
            });
        }
        idx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_size_clamps() {
        let policy = ExecutionPolicy::default();
        let plan = SamplePlan::from_policy(&policy);
        let summary = GlobalStatisticsSummary::default();
        let b = GlobalAnalyzer::block_size(&summary, &policy, &plan, 4);
        assert!(b >= policy.min_block_bytes);
        assert!(b <= policy.max_block_bytes);
    }
}
