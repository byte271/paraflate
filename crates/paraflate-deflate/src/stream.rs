use paraflate_core::{DeflateStrategy, ParaflateError, ParaflateResult};
use paraflate_lz77::Lz77Token;

use crate::bit_writer::BitWriter;
use crate::dynamic_block::{
    aggregate_freq, build_dynamic_trees, encode_dynamic_block, encode_dynamic_block_fresh,
};
use crate::fixed::encode_fixed_block;

#[derive(Clone, Debug)]
pub struct DeflateEncodeOptions {
    pub global_huffman: bool,
}

impl Default for DeflateEncodeOptions {
    fn default() -> Self {
        Self {
            global_huffman: false,
        }
    }
}

fn pm_err(e: crate::pm::PmError) -> ParaflateError {
    ParaflateError::CompressionFailed(format!("Huffman build failed: {e:?}"))
}

pub fn encode_deflate_blocks(
    blocks: &[Vec<Lz77Token>],
    strategy: DeflateStrategy,
    opts: &DeflateEncodeOptions,
) -> ParaflateResult<Vec<u8>> {
    let total_tokens: usize = blocks.iter().map(|b| b.len()).sum();
    if total_tokens > (1usize << 28) {
        return Err(ParaflateError::CompressionFailed(
            "token stream too large".to_string(),
        ));
    }
    let mut w = BitWriter::new();
    match strategy {
        DeflateStrategy::Fixed => {
            let mut merged = Vec::with_capacity(total_tokens);
            for b in blocks {
                merged.extend_from_slice(b);
            }
            encode_fixed_block(&mut w, &merged, true);
        }
        _ => {
            let global_trees = if opts.global_huffman && blocks.len() > 1 {
                let (lf, df) = aggregate_freq(blocks);
                Some(build_dynamic_trees(&lf, &df).map_err(pm_err)?)
            } else {
                None
            };
            if blocks.is_empty() {
                encode_dynamic_block_fresh(&mut w, &[], true).map_err(pm_err)?;
            } else {
                let n = blocks.len();
                for (i, b) in blocks.iter().enumerate() {
                    let bfinal = i + 1 == n;
                    if let Some(ref tr) = global_trees {
                        encode_dynamic_block(&mut w, b, tr, bfinal).map_err(pm_err)?;
                    } else {
                        encode_dynamic_block_fresh(&mut w, b, bfinal).map_err(pm_err)?;
                    }
                }
            }
        }
    }
    Ok(w.finish())
}

pub fn encode_one_deflate_block(
    tokens: &[Lz77Token],
    strategy: DeflateStrategy,
    _opts: &DeflateEncodeOptions,
    bfinal: bool,
) -> ParaflateResult<Vec<u8>> {
    if tokens.len() > (1usize << 28) {
        return Err(ParaflateError::CompressionFailed(
            "token stream too large".to_string(),
        ));
    }
    let mut w = BitWriter::new();
    match strategy {
        DeflateStrategy::Fixed => encode_fixed_block(&mut w, tokens, bfinal),
        _ => encode_dynamic_block_fresh(&mut w, tokens, bfinal).map_err(pm_err)?,
    }
    Ok(w.finish())
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use flate2::read::DeflateDecoder;
    use paraflate_core::DeflateStrategy;
    use paraflate_lz77::{compress_block, Lz77BlockParams, Lz77Config};

    use super::*;

    fn lz77_tokens(buf: &[u8]) -> Vec<Lz77Token> {
        let params = Lz77BlockParams {
            entry: 0,
            entry_rel_base: 0,
            emit_start: 0,
            emit_end: buf.len(),
        };
        compress_block(buf, &params, &Lz77Config::default(), None).tokens
    }

    #[test]
    fn dynamic_deflate_roundtrips_with_flate2() {
        let buf: Vec<u8> = (0u16..6000).map(|i| (i % 251) as u8).collect();
        let toks = lz77_tokens(&buf);
        let enc = encode_deflate_blocks(
            &[toks],
            DeflateStrategy::Default,
            &DeflateEncodeOptions::default(),
        )
        .unwrap();
        let mut dec = DeflateDecoder::new(&enc[..]);
        let mut out = Vec::new();
        dec.read_to_end(&mut out).unwrap();
        assert_eq!(out, buf);
    }

    #[test]
    fn multi_block_global_huffman_roundtrips() {
        let a = vec![b'x'; 2000];
        let b = vec![b'y'; 2000];
        let t0 = lz77_tokens(&a);
        let t1 = lz77_tokens(&b);
        let enc = encode_deflate_blocks(
            &[t0, t1],
            DeflateStrategy::Default,
            &DeflateEncodeOptions {
                global_huffman: true,
            },
        )
        .unwrap();
        let mut dec = DeflateDecoder::new(&enc[..]);
        let mut out = Vec::new();
        dec.read_to_end(&mut out).unwrap();
        let mut expect = a;
        expect.extend_from_slice(&b);
        assert_eq!(out, expect);
    }
}
