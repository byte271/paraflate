pub const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
    131, 163, 195, 227, 258,
];

pub const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];

pub const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];

pub const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
    13, 13,
];

// ---------------------------------------------------------------------------
// O(1) lookup tables built at compile time.
// ---------------------------------------------------------------------------

/// Maps length value (3..=258) → (symbol 257..=285, extra_bits).
/// Index 0 = length 3, index 255 = length 258.
static LENGTH_CODE_TABLE: std::sync::OnceLock<Box<[(u16, u8); 259]>> =
    std::sync::OnceLock::new();

/// Maps distance value (1..=32768) → (symbol 0..=29, extra_bits).
/// Stored as a flat array indexed by distance-1 (0..=32767).
static DIST_CODE_TABLE: std::sync::OnceLock<Box<[(u16, u8); 32768]>> =
    std::sync::OnceLock::new();

fn build_length_table() -> Box<[(u16, u8); 259]> {
    // Safety: we fill every index 3..=258 below; indices 0..2 are never used.
    let mut t = Box::new([(0u16, 0u8); 259]);
    for i in 0..29usize {
        let base = LENGTH_BASE[i] as usize;
        let extra = LENGTH_EXTRA[i];
        let hi = base + ((1usize << extra) - 1);
        for len in base..=hi {
            if len <= 258 {
                t[len] = ((257 + i) as u16, extra);
            }
        }
    }
    // length 258 is special: symbol 285, 0 extra bits.
    t[258] = (285, 0);
    t
}

fn build_dist_table() -> Box<[(u16, u8); 32768]> {
    let mut t = Box::new([(0u16, 0u8); 32768]);
    for i in 0..30usize {
        let base = DIST_BASE[i] as usize;
        let extra = DIST_EXTRA[i];
        let hi = (base + ((1usize << extra) - 1)).min(32768);
        for dist in base..=hi {
            t[dist - 1] = (i as u16, extra);
        }
    }
    t
}

#[inline(always)]
pub fn length_code(len: usize) -> (u16, u8) {
    debug_assert!(len >= 3 && len <= 258);
    let t = LENGTH_CODE_TABLE.get_or_init(build_length_table);
    t[len]
}

#[inline(always)]
pub fn distance_code(dist: usize) -> (u16, u8) {
    debug_assert!(dist >= 1 && dist <= 32768);
    let t = DIST_CODE_TABLE.get_or_init(build_dist_table);
    t[dist - 1]
}
