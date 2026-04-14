use std::collections::HashMap;

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

pub struct PatternIndex {
    per_entry: Vec<HashMap<u32, Vec<Occurrence>>>,
    min_match: usize,
}

impl PatternIndex {
    pub fn build(entries: &[(u32, &[u8])], cfg: &IndexBuildConfig) -> Self {
        let max_id = entries.iter().map(|(i, _)| *i).max().unwrap_or(0) as usize;
        let mut per_entry: Vec<HashMap<u32, Vec<Occurrence>>> =
            (0..=max_id).map(|_| HashMap::new()).collect();
        let m = cfg.min_match.max(3);
        for (eid, data) in entries {
            let e = *eid as usize;
            if e >= per_entry.len() {
                continue;
            }
            if data.len() < m {
                continue;
            }
            let mut o = 0usize;
            while o + m <= data.len() {
                let h = roll_hash3(data[o], data[o + 1], data[o + 2]);
                let ent = &mut per_entry[e];
                let v = ent.entry(h).or_insert_with(Vec::new);
                v.push(Occurrence { offset: o as u64 });
                if v.len() > cfg.max_chain {
                    v.remove(0);
                }
                o = o.saturating_add(cfg.stride.max(1));
            }
        }
        Self {
            per_entry,
            min_match: m,
        }
    }

    pub fn empty() -> Self {
        Self {
            per_entry: Vec::new(),
            min_match: 3,
        }
    }

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
        if entry as usize >= self.per_entry.len() {
            return None;
        }
        let m = self.min_match;
        if pos + m > buf.len() {
            return None;
        }
        let h = roll_hash3(buf[pos], buf[pos + 1], buf[pos + 2]);
        let ent = &self.per_entry[entry as usize];
        let list = ent.get(&h)?;
        let mut best: Option<(usize, usize)> = None;
        for occ in list.iter().rev() {
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
                len = len.saturating_add(1);
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

fn roll_hash3(b0: u8, b1: u8, b2: u8) -> u32 {
    (b0 as u32)
        .wrapping_shl(16)
        .wrapping_add((b1 as u32).wrapping_shl(8))
        .wrapping_add(b2 as u32)
        .wrapping_mul(0x9E37_79B9u32)
}
