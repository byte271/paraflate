use paraflate_core::{ParaflateError, ParaflateResult};

use crate::bit_reader::BitReader;
use crate::huffman::{
    build_decode_tree, decode_symbol, fixed_dist_lengths, fixed_lit_lengths, HuffNode,
};
use crate::tables::{CL_ORDER, DIST_BASE, DIST_EXTRA, LENGTH_BASE, LENGTH_EXTRA};

pub fn inflate_raw_stream(
    data: &[u8],
    expected_uncompressed: Option<usize>,
) -> ParaflateResult<Vec<u8>> {
    let mut br = BitReader::new(data);
    let mut out = Vec::new();
    let mut final_block = false;
    while !final_block {
        final_block = br.read_bits(1).map_err(|_| deflate_err())? != 0;
        let btype = br.read_bits(2).map_err(|_| deflate_err())?;
        match btype {
            0 => {
                br.align_to_byte_boundary().map_err(|_| deflate_err())?;
                let len = br.read_bits(16).map_err(|_| deflate_err())? as u16;
                let nlen = br.read_bits(16).map_err(|_| deflate_err())? as u16;
                if (len ^ nlen) != 0xFFFF {
                    return Err(deflate_err());
                }
                for _ in 0..len {
                    let b = *br.bytes.get(br.pos).ok_or_else(deflate_err)?;
                    br.pos += 1;
                    out.push(b);
                }
            }
            1 => {
                let lit_tree =
                    build_decode_tree(&fixed_lit_lengths(), 288).map_err(|_| deflate_err())?;
                let dist_tree =
                    build_decode_tree(&fixed_dist_lengths(), 30).map_err(|_| deflate_err())?;
                inflate_huff_block(&mut br, &lit_tree, &dist_tree, &mut out)?;
            }
            2 => {
                let hlit = br.read_bits(5).map_err(|_| deflate_err())? as usize + 257;
                let hdist = br.read_bits(5).map_err(|_| deflate_err())? as usize + 1;
                let hclen = br.read_bits(4).map_err(|_| deflate_err())? as usize + 4;
                let mut clen_lens = [0u8; 19];
                for i in 0..hclen {
                    let v = br.read_bits(3).map_err(|_| deflate_err())? as u8;
                    clen_lens[CL_ORDER[i]] = v;
                }
                let cl_tree = build_decode_tree(&clen_lens, 19).map_err(|_| deflate_err())?;
                let mut lit_dist_lens = vec![0u8; hlit + hdist];
                let mut sym_i = 0usize;
                while sym_i < hlit + hdist {
                    let s = decode_symbol(&cl_tree, &mut br).map_err(|_| deflate_err())?;
                    match s {
                        0..=15 => {
                            lit_dist_lens[sym_i] = s as u8;
                            sym_i += 1;
                        }
                        16 => {
                            let prev = if sym_i == 0 {
                                0
                            } else {
                                lit_dist_lens[sym_i - 1]
                            };
                            let rep = 3 + br.read_bits(2).map_err(|_| deflate_err())?;
                            for _ in 0..rep {
                                if sym_i >= lit_dist_lens.len() {
                                    return Err(deflate_err());
                                }
                                lit_dist_lens[sym_i] = prev;
                                sym_i += 1;
                            }
                        }
                        17 => {
                            let rep = 3 + br.read_bits(3).map_err(|_| deflate_err())?;
                            for _ in 0..rep {
                                if sym_i >= lit_dist_lens.len() {
                                    return Err(deflate_err());
                                }
                                lit_dist_lens[sym_i] = 0;
                                sym_i += 1;
                            }
                        }
                        18 => {
                            let rep = 11 + br.read_bits(7).map_err(|_| deflate_err())?;
                            for _ in 0..rep {
                                if sym_i >= lit_dist_lens.len() {
                                    return Err(deflate_err());
                                }
                                lit_dist_lens[sym_i] = 0;
                                sym_i += 1;
                            }
                        }
                        _ => return Err(deflate_err()),
                    }
                }
                if lit_dist_lens.len() < hlit + hdist {
                    return Err(deflate_err());
                }
                let lit_lens = &lit_dist_lens[..hlit];
                let dist_lens = &lit_dist_lens[hlit..hlit + hdist];
                if lit_lens[256] == 0 {
                    return Err(deflate_err());
                }
                let lit_tree = build_decode_tree(lit_lens, hlit).map_err(|_| deflate_err())?;
                let dist_tree = build_decode_tree(dist_lens, hdist).map_err(|_| deflate_err())?;
                inflate_huff_block(&mut br, &lit_tree, &dist_tree, &mut out)?;
            }
            _ => return Err(deflate_err()),
        }
    }
    if let Some(exp) = expected_uncompressed {
        if out.len() != exp {
            return Err(ParaflateError::VerificationFailed {
                message: format!("inflate len {} expected {}", out.len(), exp),
                entry: None,
            });
        }
    }
    Ok(out)
}

fn deflate_err() -> ParaflateError {
    ParaflateError::ZipStructure("invalid deflate stream".to_string())
}

fn inflate_huff_block(
    br: &mut BitReader,
    lit_tree: &[HuffNode],
    dist_tree: &[HuffNode],
    out: &mut Vec<u8>,
) -> ParaflateResult<()> {
    loop {
        let sym = decode_symbol(lit_tree, br).map_err(|_| deflate_err())?;
        if sym == 256 {
            break;
        }
        if sym < 256 {
            out.push(sym as u8);
            continue;
        }
        let li = (sym as usize).saturating_sub(257);
        if li >= 29 {
            return Err(deflate_err());
        }
        let extra_l = LENGTH_EXTRA[li] as u8;
        let base_l = LENGTH_BASE[li] as usize;
        let extra = br.read_bits(extra_l).map_err(|_| deflate_err())? as usize;
        let length = base_l + extra;
        let dsym = decode_symbol(dist_tree, br).map_err(|_| deflate_err())? as usize;
        if dsym >= 30 {
            return Err(deflate_err());
        }
        let extra_d = DIST_EXTRA[dsym] as u8;
        let base_d = DIST_BASE[dsym] as usize;
        let extra2 = br.read_bits(extra_d).map_err(|_| deflate_err())? as usize;
        let dist = base_d + extra2;
        if dist == 0 || dist > 32768 || dist > out.len() {
            return Err(deflate_err());
        }
        for _ in 0..length {
            let b = out[out.len() - dist];
            out.push(b);
        }
    }
    Ok(())
}
