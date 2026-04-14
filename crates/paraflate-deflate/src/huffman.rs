pub fn reverse_bits(mut code: u32, len: u8) -> u32 {
    let mut out = 0u32;
    let mut i = 0u8;
    while i < len {
        out = (out << 1) | (code & 1);
        code >>= 1;
        i += 1;
    }
    out
}

pub fn canonical_codes(lengths: &[u8], max_bits: u8) -> Vec<(u32, u8)> {
    let n = lengths.len();
    let mut bl_count = vec![0u16; (max_bits as usize) + 1];
    for &l in lengths {
        if l as usize > max_bits as usize {
            continue;
        }
        if l > 0 {
            bl_count[l as usize] += 1;
        }
    }
    let mut code = 0u32;
    let mut next_code = vec![0u32; (max_bits as usize) + 1];
    for bits in 1..=max_bits as usize {
        code = (code + bl_count[bits - 1] as u32) << 1;
        next_code[bits] = code;
    }
    let mut out = vec![(0u32, 0u8); n];
    for sym in 0..n {
        let l = lengths[sym];
        if l == 0 {
            continue;
        }
        let c = next_code[l as usize];
        next_code[l as usize] += 1;
        let rev = reverse_bits(c, l);
        out[sym] = (rev, l);
    }
    out
}
