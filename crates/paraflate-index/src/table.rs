/// 15-bit hash mask — same as the LZ77 window hash table.
const HASH_SLOTS: usize = 1 << 15; // 32768
const HASH_MASK: usize = HASH_SLOTS - 1;

#[derive(Clone, Debug)]
pub struct IndexBuildConfig {
    pub min_match: usize,
    pub stride: usize,
    pub max_chain: usize,
}

impl Default for IndexBuildConfig {
    fn default() -> Self {
        Self {
            min_match: 3,
            stride: 1,
            max_chain: 96,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Occurrence {
    pub offset: u64,
}

/// Per-entry pattern index backed by a flat array instead of HashMap.
/// Each slot holds a small Vec of occurrences (capped at `max_chain`).
/// Direct array indexing is ~3× faster than HashMap for 15-bit keys.
pub struct PatternIndex {
    /// One flat table per entry: `per_entry[entry_id][hash_slot]`
    per_entry: Vec<Box<[Vec<Occurrence>]>>,
    min_match: usize,
}

impl PatternIndex {
    pub fn build(entries: &[(u32, &[u8])], cfg: &IndexBuildConfig) -> Self {
        if entries.is_empty() {
            return Self::empty();
        }
        let max_id = entries.iter().map(|(i, _)| *i).max().unwrap_or(0) as usize;
        let m = cfg.min_match.max(3);
        let cap = cfg.max_chain.max(1);

        // Allocate all tables up-front as flat boxed slices.
        let mut per_entry: Vec<Box<[Vec<Occurrence>]>> = (0..=max_id)
            .map(|_| {
                let v: Vec<Vec<Occurrence>> = (0..HASH_SLOTS).map(|_| Vec::new()).collect();
                v.into_boxed_slice()
            })
            .collect();

        for (eid, data) in entries {
            let e = *eid as usize;
            if e >= per_entry.len() || data.len() < m {
                continue;
            }
            let table = &mut per_entry[e];
            let stride = cfg.stride.max(1);
            let mut o = 0usize;
            while o + m <= data.len() {
                let h = hash3(data[o], data[o + 1], data[o + 2]) & HASH_MASK;
                let slot = &mut table[h];
                if slot.len() >= cap {
                    // Evict oldest (front) to keep the most recent matches.
                    slot.remove(0);
                }
                slot.push(Occurrence { offset: o as u64 });
                o += stride;
            }
        }

        Self { per_entry, min_match: m }
    }

    pub fn empty() -> Self {
        Self {
            per_entry: Vec::new(),
            min_match: 3,
        }
    }

    #[inline]
    pub fn scan_global(
        &self,
        entry: u32,
        cur_rel: u64,
        buf_rel_base: u64,
        buf: &[u8],
        pos: usize,
        max_dist: usize,
        min_len: usize,
    ) -> Option<(usize, usize)> {
        let e = entry as usize;
        if e >= self.per_entry.len() {
            return None;
        }
        let m = self.min_match;
        if pos + m > buf.len() {
            return None;
        }
        let h = hash3(buf[pos], buf[pos + 1], buf[pos + 2]) & HASH_MASK;
        let slot = &self.per_entry[e][h];
        if slot.is_empty() {
            return None;
        }

        let mut best: Option<(usize, usize)> = None;
        // Iterate in reverse so we see the most recent (closest) matches first.
        for occ in slot.iter().rev() {
            if occ.offset >= cur_rel {
                continue;
            }
            let dist = (cur_rel - occ.offset) as usize;
            if dist == 0 || dist > max_dist {
                continue;
            }
            let rel = occ.offset as i128 - buf_rel_base as i128;
            if rel < 0 {
                continue;
            }
            let mp = rel as usize;
            if mp >= pos {
                continue;
            }
            let mut len = 0usize;
            while pos + len < buf.len()
                && mp + len < pos
                && buf[mp + len] == buf[pos + len]
                && len < 258
            {
                len += 1;
            }
            if len >= min_len {
                let score = len.saturating_mul(65536).saturating_sub(dist);
                let take = match best {
                    None => true,
                    Some((bl, bd)) => {
                        let old = bl.saturating_mul(65536).saturating_sub(bd);
                        score > old || (score == old && dist < bd)
                    }
                };
                if take {
                    best = Some((len, dist));
                }
            }
        }
        best
    }
}

#[inline(always)]
fn hash3(b0: u8, b1: u8, b2: u8) -> usize {
    let x = (b0 as u32)
        .wrapping_shl(16)
        .wrapping_add((b1 as u32).wrapping_shl(8))
        .wrapping_add(b2 as u32)
        .wrapping_mul(0x9E37_79B9u32);
    x as usize
}
