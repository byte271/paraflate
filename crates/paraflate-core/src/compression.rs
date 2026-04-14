#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompressionMethod {
    Stored,
    Deflate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeflateStrategy {
    Default,
    Filtered,
    HuffmanOnly,
    Rle,
    Fixed,
}

#[derive(Clone, Debug, Default)]
pub struct DeflateBlockDebugRecord {
    pub entry_id: u32,
    pub block_id: u64,
    pub offset: u64,
    pub raw_span_bytes: u64,
    pub compressed_bytes: u64,
    pub token_count: usize,
    pub predicted_ratio: f64,
    pub actual_ratio: f64,
    pub lz_matches_window: u32,
    pub lz_matches_index: u32,
    pub fallback_code: u8,
}

#[derive(Clone, Debug)]
pub struct CompressionProfile {
    pub method: CompressionMethod,
    pub level: u32,
    pub strategy: DeflateStrategy,
    pub window_bits: u8,
    pub global_huffman: bool,
}

impl Default for CompressionProfile {
    fn default() -> Self {
        Self {
            method: CompressionMethod::Deflate,
            level: 6,
            strategy: DeflateStrategy::Default,
            window_bits: 15,
            global_huffman: false,
        }
    }
}
