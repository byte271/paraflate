pub const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];

pub const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];

pub const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];

pub const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

pub fn length_code(len: usize) -> (u16, u8) {
    debug_assert!(len >= 3 && len <= 258);
    for i in 0..29 {
        let base = LENGTH_BASE[i] as usize;
        let extra = LENGTH_EXTRA[i] as usize;
        let hi = base + ((1usize << extra) - 1);
        if len >= base && len <= hi {
            return ((257 + i) as u16, LENGTH_EXTRA[i]);
        }
    }
    (285, 0)
}

pub fn distance_code(dist: usize) -> (u16, u8) {
    debug_assert!(dist >= 1 && dist <= 32768);
    for i in 0..30 {
        let base = DIST_BASE[i] as usize;
        let extra = DIST_EXTRA[i] as usize;
        let hi = base + ((1usize << extra) - 1);
        if dist >= base && dist <= hi {
            return ((i as u16), DIST_EXTRA[i]);
        }
    }
    (29, 13)
}
