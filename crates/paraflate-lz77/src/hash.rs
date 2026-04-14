pub fn roll_hash3(b0: u8, b1: u8, b2: u8) -> u32 {
    let x = (b0 as u32)
        .wrapping_shl(16)
        .wrapping_add((b1 as u32).wrapping_shl(8))
        .wrapping_add(b2 as u32);
    x.wrapping_mul(0x9E37_79B9u32)
}
