#[derive(Clone, Copy, Debug, Default)]
pub struct ByteClassHistogram {
    pub zero: u64,
    pub ascii_text: u64,
    pub high_entropy: u64,
    pub duplicate_window_hits: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PatternDigest(pub u64);

#[derive(Clone, Debug, Default)]
pub struct GlobalStatisticsSummary {
    pub total_uncompressed: u64,
    pub entry_count: u64,
    pub histogram: ByteClassHistogram,
    pub top_patterns: Vec<(PatternDigest, u32)>,
    pub mean_entry_bytes: u64,
    pub duplicate_mass: u64,
    pub text_mass: u64,
}
