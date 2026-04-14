use paraflate_lz77::Lz77Token;

use crate::bit_writer::BitWriter;
use crate::dynamic::length_limited_lengths;
use crate::huffman::canonical_codes;
use crate::pm::PmError;
use crate::tables::{distance_code, length_code};

const CL_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

#[derive(Clone, Debug)]
pub struct DynamicTrees {
    pub lit_len: Vec<u8>,
    pub dist: Vec<u8>,
    pub lit_codes: Vec<(u32, u8)>,
    pub dist_codes: Vec<(u32, u8)>,
}

pub fn count_block_freq(tokens: &[Lz77Token]) -> ([u64; 286], [u64; 30]) {
    let mut lit = [0u64; 286];
    let mut dist = [0u64; 30];
    for t in tokens {
        match t {
            Lz77Token::Literal(b) => {
                lit[*b as usize] = lit[*b as usize].saturating_add(1);
            }
            Lz77Token::Match { length, distance } => {
                let len = *length as usize;
                let distv = *distance as usize;
                let (sym, _) = length_code(len);
                lit[sym as usize] = lit[sym as usize].saturating_add(1);
                let (dsym, _) = distance_code(distv);
                dist[dsym as usize] = dist[dsym as usize].saturating_add(1);
            }
        }
    }
    lit[256] = lit[256].saturating_add(1);
    (lit, dist)
}

pub fn aggregate_freq(blocks: &[Vec<Lz77Token>]) -> ([u64; 286], [u64; 30]) {
    let mut lit = [0u64; 286];
    let mut dist = [0u64; 30];
    for b in blocks {
        let (l, d) = count_block_freq(b);
        for i in 0..286 {
            lit[i] = lit[i].saturating_add(l[i]);
        }
        for i in 0..30 {
            dist[i] = dist[i].saturating_add(d[i]);
        }
    }
    (lit, dist)
}

fn huffman_lengths(freq: &[u64], max_bits: u32, alphabet_size: usize) -> Result<Vec<u8>, PmError> {
    let mut out = vec![0u8; alphabet_size];
    let active: Vec<usize> = (0..alphabet_size).filter(|&i| freq[i] > 0).collect();
    if active.is_empty() {
        return Err(PmError::NoSymbols);
    }
    if active.len() == 1 {
        out[active[0]] = 1;
        return Ok(out);
    }
    let f: Vec<u64> = active.iter().map(|&i| freq[i]).collect();
    let part = length_limited_lengths(&f, max_bits)?;
    for (j, &sym) in active.iter().enumerate() {
        out[sym] = part[j];
    }
    Ok(out)
}

fn trim_lit(mut lit: Vec<u8>) -> Vec<u8> {
    while lit.len() > 257 && lit[lit.len() - 1] == 0 {
        lit.pop();
    }
    lit
}

fn trim_dist(mut dist: Vec<u8>) -> Vec<u8> {
    while dist.len() > 1 && dist[dist.len() - 1] == 0 {
        dist.pop();
    }
    if dist.is_empty() {
        dist.push(0);
    }
    dist
}

fn cl_rle_tokens(lengths: &[u8]) -> Vec<(u8, u32, u8)> {
    let mut n = 0usize;
    let mut prev: u8 = 0;
    let mut out = Vec::new();
    while n < lengths.len() {
        let c = lengths[n];
        let mut run = 0usize;
        while n + run < lengths.len() && lengths[n + run] == c {
            run += 1;
        }
        if c == 0 {
            let mut rem = run;
            while rem >= 11 {
                let k = rem.min(138);
                out.push((18, (k - 11) as u32, 7));
                rem -= k;
            }
            if rem >= 3 {
                out.push((17, (rem - 3) as u32, 3));
                rem = 0;
            }
            for _ in 0..rem {
                out.push((0, 0, 0));
            }
            prev = 0;
            n += run;
            continue;
        }
        if c == prev && run >= 3 {
            let mut rem = run;
            while rem >= 3 {
                let k = rem.min(6);
                out.push((16, (k - 3) as u32, 2));
                rem -= k;
            }
            for _ in 0..rem {
                out.push((c, 0, 0));
                prev = c;
            }
            n += run;
            continue;
        }
        for _ in 0..run {
            out.push((c, 0, 0));
            prev = c;
            n += 1;
        }
    }
    out
}

fn count_cl_freq(tokens: &[(u8, u32, u8)]) -> [u64; 19] {
    let mut f = [0u64; 19];
    for (sym, _, _) in tokens {
        f[*sym as usize] = f[*sym as usize].saturating_add(1);
    }
    f
}

fn ensure_two_symbols(freq: &mut [u64; 19]) {
    let nz: Vec<usize> = (0..19).filter(|&i| freq[i] > 0).collect();
    if nz.len() >= 2 {
        return;
    }
    if nz.is_empty() {
        freq[0] = 1;
        freq[1] = 1;
        return;
    }
    let a = nz[0];
    let b = if a == 0 { 1 } else { 0 };
    freq[b] = freq[b].saturating_add(1);
}

fn hclen_plus4(cl_lens: &[u8; 19]) -> usize {
    let mut last = 3usize;
    for i in 0..19 {
        let sym = CL_ORDER[i];
        if cl_lens[sym] != 0 {
            last = i;
        }
    }
    last.max(3) + 1
}

pub fn build_dynamic_trees(
    lit_freq: &[u64; 286],
    dist_freq: &[u64; 30],
) -> Result<DynamicTrees, PmError> {
    let mut lit_freq = *lit_freq;
    let mut dist_freq = *dist_freq;
    if lit_freq[256] == 0 {
        lit_freq[256] = 1;
    }
    let dist_sum: u64 = dist_freq.iter().sum();
    if dist_sum == 0 {
        dist_freq[0] = 1;
    }
    let lit_len = huffman_lengths(&lit_freq[..], 15, 286)?;
    let dist_len = huffman_lengths(&dist_freq[..], 15, 30)?;
    let lit_trim = trim_lit(lit_len);
    let dist_trim = trim_dist(dist_len);
    let lit_codes = canonical_codes(&lit_trim, 15);
    let dist_codes = canonical_codes(&dist_trim, 15);
    Ok(DynamicTrees {
        lit_len: lit_trim,
        dist: dist_trim,
        lit_codes,
        dist_codes,
    })
}

pub fn encode_dynamic_block(
    w: &mut BitWriter,
    tokens: &[Lz77Token],
    trees: &DynamicTrees,
    bfinal: bool,
) -> Result<(), PmError> {
    let hdr = (bfinal as u32) | (2u32 << 1);
    w.put_bits(hdr, 3);
    let hlit = (trees.lit_len.len() - 257) as u32;
    let hdist = (trees.dist.len() - 1) as u32;
    debug_assert!(hlit <= 29);
    debug_assert!(hdist <= 29);
    let mut concat = Vec::with_capacity(trees.lit_len.len() + trees.dist.len());
    concat.extend_from_slice(&trees.lit_len);
    concat.extend_from_slice(&trees.dist);
    let cl_tokens = cl_rle_tokens(&concat);
    let mut cl_freq = count_cl_freq(&cl_tokens);
    ensure_two_symbols(&mut cl_freq);
    let cl_lens_arr = huffman_lengths(&cl_freq[..], 7, 19)?;
    let mut cl_lens = [0u8; 19];
    for i in 0..19 {
        cl_lens[i] = cl_lens_arr[i];
    }
    let cl_codes = canonical_codes(&cl_lens_arr, 7);
    let m = hclen_plus4(&cl_lens) as u32;
    w.put_bits(hlit, 5);
    w.put_bits(hdist, 5);
    w.put_bits(m - 4, 4);
    for i in 0..(m as usize) {
        let sym = CL_ORDER[i];
        w.put_bits(cl_lens[sym] as u32, 3);
    }
    for (sym, extra, xbits) in &cl_tokens {
        let (c, l) = cl_codes[*sym as usize];
        w.put_bits(c, l);
        if *xbits > 0 {
            w.put_bits(*extra, *xbits);
        }
    }
    for t in tokens {
        match t {
            Lz77Token::Literal(b) => {
                let (c, l) = trees.lit_codes[*b as usize];
                w.put_bits(c, l);
            }
            Lz77Token::Match { length, distance } => {
                let len = *length as usize;
                let distv = *distance as usize;
                let (sym, extra_len) = length_code(len);
                let (c, l) = trees.lit_codes[sym as usize];
                w.put_bits(c, l);
                if extra_len > 0 {
                    let base = crate::tables::LENGTH_BASE[(sym - 257) as usize] as usize;
                    let extra = (len - base) as u32;
                    w.put_bits(extra, extra_len);
                }
                let (dsym, dextra_len) = distance_code(distv);
                let (dc, dl) = trees.dist_codes[dsym as usize];
                w.put_bits(dc, dl);
                if dextra_len > 0 {
                    let base = crate::tables::DIST_BASE[dsym as usize] as usize;
                    let extra = (distv - base) as u32;
                    w.put_bits(extra, dextra_len);
                }
            }
        }
    }
    let (c, l) = trees.lit_codes[256];
    w.put_bits(c, l);
    Ok(())
}

pub fn encode_dynamic_block_fresh(
    w: &mut BitWriter,
    tokens: &[Lz77Token],
    bfinal: bool,
) -> Result<(), PmError> {
    let (lf, df) = count_block_freq(tokens);
    let trees = build_dynamic_trees(&lf, &df)?;
    encode_dynamic_block(w, tokens, &trees, bfinal)
}
