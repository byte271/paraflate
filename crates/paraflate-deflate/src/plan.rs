use paraflate_core::{BlockId, BlockSpan, ChunkPlan, EntryId, ExecutionPolicy};
use paraflate_dictionary::GlobalModel;

pub struct BlockPlanner;

impl BlockPlanner {
    pub fn plan_entry(
        entry: EntryId,
        uncompressed: u64,
        policy: &ExecutionPolicy,
        model: &GlobalModel,
    ) -> ChunkPlan {
        Self::plan_entry_with_data(entry, None, uncompressed, policy, model)
    }

    pub fn plan_entry_with_data(
        entry: EntryId,
        data: Option<&[u8]>,
        uncompressed: u64,
        policy: &ExecutionPolicy,
        model: &GlobalModel,
    ) -> ChunkPlan {
        Self::plan_entry_with_data_predictive(entry, data, uncompressed, policy, model, None)
    }

    pub fn plan_entry_with_data_predictive(
        entry: EntryId,
        data: Option<&[u8]>,
        uncompressed: u64,
        policy: &ExecutionPolicy,
        model: &GlobalModel,
        target_block: Option<u64>,
    ) -> ChunkPlan {
        if uncompressed == 0 {
            return ChunkPlan {
                entry,
                spans: Vec::new(),
            };
        }
        let block = target_block
            .unwrap_or(model.suggested_block_bytes.max(policy.min_block_bytes) as u64)
            .max(policy.min_block_bytes as u64)
            .min(policy.max_block_bytes as u64);
        let max_b = policy.max_block_bytes as u64;
        let min_b = policy.min_block_bytes as u64;
        let full_view = data.filter(|d| d.len() as u64 >= uncompressed);

        let mut spans = Vec::new();
        let mut offset = 0u64;
        let mut bid = 0u64;
        while offset < uncompressed {
            let remain = uncompressed - offset;
            let naive_end = offset + remain.min(block);
            let max_end = offset + remain.min(max_b);
            let split = if let (Some(buf), true) = (full_view, max_end > offset + min_b) {
                let start = offset as usize;
                let soft = naive_end.min(max_end) as usize;
                let cap = max_end as usize;
                Self::adaptive_end(buf, start, soft, cap, min_b as usize)
            } else {
                naive_end.min(max_end)
            };
            let len = split.saturating_sub(offset).max(1).min(remain);
            spans.push(BlockSpan {
                entry,
                offset,
                len,
                block: BlockId(bid),
            });
            bid = bid.saturating_add(1);
            offset = offset.saturating_add(len);
        }
        ChunkPlan { entry, spans }
    }

    fn adaptive_end(buf: &[u8], start: usize, soft: usize, cap: usize, min_b: usize) -> u64 {
        let cap = cap.min(buf.len());
        let soft = soft.min(cap);
        let lo = start.saturating_add(min_b).min(cap);
        if lo >= cap {
            return cap as u64;
        }
        let win = 4096usize;
        let mut best = soft.clamp(lo, cap);
        let mut best_score = f64::NEG_INFINITY;
        let span = cap.saturating_sub(lo);
        let step = (span / 128).max(32);
        let mut p = lo;
        while p < cap {
            let e0 = Self::normalized_entropy(buf, start, p, win);
            let e1 = Self::normalized_entropy(buf, p, cap, win);
            let r0 = Self::repeat_ratio(buf, start, p);
            let r1 = Self::repeat_ratio(buf, p, cap);
            let score = (e1 - e0).abs() + 0.35 * (r1 - r0).abs();
            if score > best_score {
                best_score = score;
                best = p;
            }
            p = p.saturating_add(step);
        }
        if !best_score.is_finite() || best_score <= f64::EPSILON {
            best = soft.clamp(lo, cap);
        }
        best as u64
    }

    fn normalized_entropy(buf: &[u8], a: usize, b: usize, max_span: usize) -> f64 {
        if b <= a + 16 {
            return 0.0;
        }
        let b = b.min(a + max_span);
        let mut cnt = [0u64; 256];
        let mut total = 0u64;
        for &x in &buf[a..b] {
            cnt[x as usize] += 1;
            total += 1;
        }
        if total == 0 {
            return 0.0;
        }
        let mut h = 0.0f64;
        for c in cnt {
            if c == 0 {
                continue;
            }
            let p = c as f64 / total as f64;
            h -= p * p.ln();
        }
        h / std::f64::consts::LN_2
    }

    fn repeat_ratio(buf: &[u8], a: usize, b: usize) -> f64 {
        if b <= a + 1 {
            return 0.0;
        }
        let mut same = 0u64;
        let mut tot = 0u64;
        for i in (a + 1)..b {
            if buf[i] == buf[i - 1] {
                same += 1;
            }
            tot += 1;
        }
        if tot == 0 {
            return 0.0;
        }
        same as f64 / tot as f64
    }
}
