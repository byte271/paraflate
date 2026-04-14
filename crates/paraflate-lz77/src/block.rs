use paraflate_index::PatternIndex;

use crate::hash::roll_hash3;
use crate::Window;

const NIL: u32 = u32::MAX;
const HASH_MASK: usize = 0x7FFF;

#[derive(Clone, Debug)]
pub struct Lz77Config {
    pub max_chain: usize,
    pub nice_match: usize,
}

impl Default for Lz77Config {
    fn default() -> Self {
        Self {
            max_chain: 256,
            nice_match: 258,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Lz77BlockParams {
    pub entry: u32,
    pub entry_rel_base: u64,
    pub emit_start: usize,
    pub emit_end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lz77Token {
    Literal(u8),
    Match { length: u16, distance: u16 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchKind {
    Window,
    Index,
}

#[derive(Clone, Debug, Default)]
pub struct Lz77CompressOutput {
    pub tokens: Vec<Lz77Token>,
    pub matches_from_window: u32,
    pub matches_from_index: u32,
}

#[inline]
fn match_score(len: usize, dist: usize) -> u64 {
    (len as u64)
        .saturating_mul(65536)
        .saturating_sub(dist as u64)
}

fn pick_match(
    index_match: Option<(usize, usize)>,
    window_match: Option<(usize, usize)>,
) -> Option<(usize, usize, MatchKind)> {
    match (index_match, window_match) {
        (None, None) => None,
        (None, Some((l, d))) => Some((l, d, MatchKind::Window)),
        (Some((l, d)), None) => Some((l, d, MatchKind::Index)),
        (Some((li, di)), Some((lw, dw))) => {
            let si = match_score(li, di);
            let sw = match_score(lw, dw);
            if si > sw || si == sw {
                Some((li, di, MatchKind::Index))
            } else {
                Some((lw, dw, MatchKind::Window))
            }
        }
    }
}

fn best_at(
    buf: &[u8],
    pos: usize,
    head: &[u32],
    prev: &[u32],
    cfg: &Lz77Config,
    index: Option<&PatternIndex>,
    entry: u32,
    entry_rel_base: u64,
) -> Option<(usize, usize, MatchKind)> {
    if pos + 3 > buf.len() {
        return None;
    }
    let h = (roll_hash3(buf[pos], buf[pos + 1], buf[pos + 2]) as usize) & HASH_MASK;
    let max_dist = 32768usize.min(pos);
    let cur_rel = entry_rel_base.saturating_add(pos as u64);
    let mut window_best: Option<(usize, usize)> = None;
    let mut chain = 0usize;
    let mut cur = head[h];
    while cur != NIL && chain < cfg.max_chain {
        let mp = cur as usize;
        if mp >= pos {
            break;
        }
        let dist = pos - mp;
        if dist == 0 || dist > 32768 || dist > max_dist {
            break;
        }
        let mut len = 0usize;
        while pos + len < buf.len()
            && mp + len < pos
            && buf[mp + len] == buf[pos + len]
            && len < 258
        {
            len += 1;
        }
        if len >= 3 {
            let score = match_score(len, dist);
            let take = match window_best {
                None => true,
                Some((bl, bd)) => {
                    let old = match_score(bl, bd);
                    score > old || (score == old && dist < bd)
                }
            };
            if take {
                window_best = Some((len, dist));
            }
            if len >= cfg.nice_match {
                break;
            }
        }
        cur = prev[mp];
        chain += 1;
    }
    let index_best = index.and_then(|ix| {
        ix.scan_global(
            entry,
            cur_rel,
            entry_rel_base,
            buf,
            pos,
            max_dist.min(32768),
            3,
        )
    });
    pick_match(index_best, window_best)
}

fn insert_hash(buf: &[u8], pos: usize, head: &mut [u32], prev: &mut [u32]) {
    if pos + 3 > buf.len() {
        return;
    }
    let h = (roll_hash3(buf[pos], buf[pos + 1], buf[pos + 2]) as usize) & HASH_MASK;
    prev[pos] = head[h];
    head[h] = pos as u32;
}

pub fn compress_block(
    buf: &[u8],
    params: &Lz77BlockParams,
    cfg: &Lz77Config,
    index: Option<&PatternIndex>,
) -> Lz77CompressOutput {
    let mut out = Lz77CompressOutput::default();
    if buf.is_empty() || params.emit_start >= params.emit_end {
        return out;
    }
    let mut head = vec![NIL; HASH_MASK + 1];
    let mut prev = vec![NIL; buf.len()];
    let win = Window::new(buf, params.entry_rel_base);
    let _ = win;
    let mut pos = params.emit_start;
    while pos < params.emit_end {
        let m0 = best_at(
            buf,
            pos,
            &head,
            &prev,
            cfg,
            index,
            params.entry,
            params.entry_rel_base,
        );
        let m1 = if pos + 1 < params.emit_end {
            best_at(
                buf,
                pos + 1,
                &head,
                &prev,
                cfg,
                index,
                params.entry,
                params.entry_rel_base,
            )
        } else {
            None
        };
        let use_literal_lazy = match (&m0, &m1) {
            (Some((l0, _, _)), Some((l1, _, _))) => *l1 > *l0,
            _ => false,
        };
        if use_literal_lazy {
            out.tokens.push(Lz77Token::Literal(buf[pos]));
            insert_hash(buf, pos, &mut head, &mut prev);
            pos += 1;
            continue;
        }
        if let Some((len, dist, src)) = m0 {
            if len >= 3 && dist >= 1 && dist <= 32768 {
                match src {
                    MatchKind::Window => {
                        out.matches_from_window = out.matches_from_window.saturating_add(1);
                    }
                    MatchKind::Index => {
                        out.matches_from_index = out.matches_from_index.saturating_add(1);
                    }
                }
                out.tokens.push(Lz77Token::Match {
                    length: len as u16,
                    distance: dist as u16,
                });
                let end = (pos + len).min(buf.len());
                let mut q = pos;
                while q < end {
                    insert_hash(buf, q, &mut head, &mut prev);
                    q += 1;
                }
                pos = end;
                continue;
            }
        }
        out.tokens.push(Lz77Token::Literal(buf[pos]));
        insert_hash(buf, pos, &mut head, &mut prev);
        pos += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_run_emits_matches() {
        let buf = vec![b'x'; 512];
        let params = Lz77BlockParams {
            entry: 0,
            entry_rel_base: 0,
            emit_start: 0,
            emit_end: buf.len(),
        };
        let cfg = Lz77Config {
            max_chain: 512,
            nice_match: 258,
        };
        let o = compress_block(&buf, &params, &cfg, None);
        assert!(o
            .tokens
            .iter()
            .any(|t| matches!(t, Lz77Token::Match { .. })));
    }

    #[test]
    fn unique_bytes_are_literals_only() {
        let buf: Vec<u8> = (0u16..200).map(|x| x as u8).collect();
        let params = Lz77BlockParams {
            entry: 1,
            entry_rel_base: 0,
            emit_start: 0,
            emit_end: buf.len(),
        };
        let o = compress_block(&buf, &params, &Lz77Config::default(), None);
        assert!(o.tokens.iter().all(|t| matches!(t, Lz77Token::Literal(_))));
    }
}
