use paraflate_index::PatternIndex;

use crate::hash::roll_hash3;
use crate::Window;

const NIL: u32 = u32::MAX;
/// 15-bit hash table — 32 768 slots, same as zlib's default.
const HASH_BITS: usize = 15;
const HASH_SIZE: usize = 1 << HASH_BITS;
const HASH_MASK: usize = HASH_SIZE - 1;

#[derive(Clone, Debug)]
pub struct Lz77Config {
    pub max_chain: usize,
    pub nice_match: usize,
}

impl Default for Lz77Config {
    fn default() -> Self {
        Self {
            max_chain: 128,
            nice_match: 128,
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

#[inline(always)]
fn match_score(len: usize, dist: usize) -> u64 {
    (len as u64)
        .saturating_mul(65536)
        .saturating_sub(dist as u64)
}

/// Extend a match starting at `(mp, pos)` as far as possible.
/// Uses 8-byte QWORD comparison for the bulk of the match.
#[inline(always)]
fn extend_match(buf: &[u8], mp: usize, pos: usize) -> usize {
    let max_len = 258usize.min(buf.len() - pos).min(pos - mp);
    let mut len = 0usize;
    while len + 8 <= max_len {
        // SAFETY: bounds checked above.
        let a = u64::from_le_bytes(buf[mp + len..mp + len + 8].try_into().unwrap());
        let b = u64::from_le_bytes(buf[pos + len..pos + len + 8].try_into().unwrap());
        let diff = a ^ b;
        if diff != 0 {
            len += (diff.trailing_zeros() / 8) as usize;
            return len;
        }
        len += 8;
    }
    while len < max_len && buf[mp + len] == buf[pos + len] {
        len += 1;
    }
    len
}

/// Find the best match at `pos` using the hash-chain window.
#[inline]
fn window_best(
    buf: &[u8],
    pos: usize,
    head: &[u32],
    prev: &[u32],
    cfg: &Lz77Config,
    h: usize,
) -> Option<(usize, usize)> {
    let max_dist = 32768usize.min(pos);
    let mut best: Option<(usize, usize)> = None;
    let mut chain = 0usize;
    let mut cur = head[h];
    while cur != NIL && chain < cfg.max_chain {
        let mp = cur as usize;
        if mp >= pos {
            break;
        }
        let dist = pos - mp;
        if dist > max_dist {
            break;
        }
        let len = extend_match(buf, mp, pos);
        if len >= 3 {
            let score = match_score(len, dist);
            let take = match best {
                None => true,
                Some((bl, bd)) => score > match_score(bl, bd),
            };
            if take {
                best = Some((len, dist));
            }
            if len >= cfg.nice_match {
                break;
            }
        }
        cur = prev[mp];
        chain += 1;
    }
    best
}

#[inline(always)]
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

    let emit_len = params.emit_end - params.emit_start;
    // Better token reserve estimate: assume ~30% match rate for typical data.
    // Literals: 1 token each, Matches: 1 token for ~10 bytes.
    // Conservative estimate: 0.7 * emit_len + 0.3 * emit_len / 10
    let est_tokens = (emit_len * 7 / 10) + (emit_len * 3 / 100);
    out.tokens.reserve(est_tokens.max(64));

    let mut head = vec![NIL; HASH_SIZE];
    let mut prev = vec![NIL; buf.len()];
    let _ = Window::new(buf, params.entry_rel_base);

    // Populate the hash table for the overlap region so that positions in
    // the emit window can find back-references into the overlap.
    // We walk forward from 0 to emit_start, inserting each position.
    let overlap_end = params.emit_start;
    let mut i = 0usize;
    while i < overlap_end {
        insert_hash(buf, i, &mut head, &mut prev);
        i += 1;
    }

    let mut pos = params.emit_start;
    while pos < params.emit_end {
        if pos + 3 > buf.len() {
            // Tail: emit remaining bytes as literals.
            while pos < params.emit_end {
                out.tokens.push(Lz77Token::Literal(buf[pos]));
                pos += 1;
            }
            break;
        }

        let h = (roll_hash3(buf[pos], buf[pos + 1], buf[pos + 2]) as usize) & HASH_MASK;

        // Window match.
        let wm = window_best(buf, pos, &head, &prev, cfg, h);

        // Index match (cross-entry). Only query if window match is weak.
        let cur_rel = params.entry_rel_base.saturating_add(pos as u64);
        let max_dist = 32768usize.min(pos);

        // Skip index query if we already have a nice_match from the window.
        let skip_index = wm.map(|(l, _)| l >= cfg.nice_match).unwrap_or(false);
        let im = if skip_index {
            None
        } else {
            index.and_then(|ix| {
                ix.scan_global(
                    params.entry,
                    cur_rel,
                    params.entry_rel_base,
                    buf,
                    pos,
                    max_dist,
                    3,
                )
            })
        };

        // Pick the better candidate.
        let best = match (wm, im) {
            (None, None) => None,
            (Some(w), None) => Some((w, MatchKind::Window)),
            (None, Some(i)) => Some((i, MatchKind::Index)),
            (Some(w), Some(i)) => {
                if match_score(i.0, i.1) >= match_score(w.0, w.1) {
                    Some((i, MatchKind::Index))
                } else {
                    Some((w, MatchKind::Window))
                }
            }
        };

        // Lazy matching: only check pos+1 if current match is below nice_match
        // and the current match is short (< 8 bytes). This avoids the overhead
        // of a second hash lookup for long matches.
        let use_literal = if let Some(((len0, _), _)) = best {
            if len0 < cfg.nice_match && len0 < 8 && pos + 1 < params.emit_end && pos + 4 <= buf.len() {
                let h1 = (roll_hash3(buf[pos + 1], buf[pos + 2], buf[pos + 3]) as usize)
                    & HASH_MASK;
                let wm1 = window_best(buf, pos + 1, &head, &prev, cfg, h1);
                let len1 = wm1.map(|(l, _)| l).unwrap_or(0);
                len1 > len0
            } else {
                false
            }
        } else {
            false
        };

        if use_literal {
            out.tokens.push(Lz77Token::Literal(buf[pos]));
            insert_hash(buf, pos, &mut head, &mut prev);
            pos += 1;
            continue;
        }

        if let Some(((len, dist), src)) = best {
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
                // Insert hashes for all positions covered by the match.
                let end = (pos + len).min(buf.len());
                for q in pos..end {
                    insert_hash(buf, q, &mut head, &mut prev);
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
