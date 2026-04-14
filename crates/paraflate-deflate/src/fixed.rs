use std::sync::OnceLock;

use paraflate_lz77::Lz77Token;

use crate::bit_writer::BitWriter;
use crate::huffman::canonical_codes;
use crate::tables::{distance_code, length_code};

static FIXED: OnceLock<(Vec<(u32, u8)>, Vec<(u32, u8)>)> = OnceLock::new();

fn fixed_tables() -> &'static (Vec<(u32, u8)>, Vec<(u32, u8)>) {
    FIXED.get_or_init(|| {
        let mut lit_len = [0u8; 288];
        for i in 0..144 {
            lit_len[i] = 8;
        }
        for i in 144..256 {
            lit_len[i] = 9;
        }
        for i in 256..280 {
            lit_len[i] = 7;
        }
        for i in 280..288 {
            lit_len[i] = 8;
        }
        let mut dist_len = [0u8; 30];
        for d in &mut dist_len {
            *d = 5;
        }
        let lc = canonical_codes(&lit_len, 15);
        let dc = canonical_codes(&dist_len, 15);
        (lc, dc)
    })
}

pub fn encode_fixed_block(w: &mut BitWriter, tokens: &[Lz77Token], bfinal: bool) {
    let hdr = (bfinal as u32) | (1u32 << 1);
    w.put_bits(hdr, 3);
    let (lit, dist) = fixed_tables();
    for t in tokens {
        match t {
            Lz77Token::Literal(b) => {
                let (c, l) = lit[*b as usize];
                w.put_bits(c, l);
            }
            Lz77Token::Match { length, distance } => {
                let len = *length as usize;
                let distv = *distance as usize;
                let (sym, extra_len) = length_code(len);
                let (c, l) = lit[sym as usize];
                w.put_bits(c, l);
                if extra_len > 0 {
                    let base = crate::tables::LENGTH_BASE[(sym - 257) as usize] as usize;
                    let extra = (len - base) as u32;
                    w.put_bits(extra, extra_len);
                }
                let (dsym, dextra_len) = distance_code(distv);
                let (dc, dl) = dist[dsym as usize];
                w.put_bits(dc, dl);
                if dextra_len > 0 {
                    let base = crate::tables::DIST_BASE[dsym as usize] as usize;
                    let extra = (distv - base) as u32;
                    w.put_bits(extra, dextra_len);
                }
            }
        }
    }
    let (c, l) = lit[256];
    w.put_bits(c, l);
}
