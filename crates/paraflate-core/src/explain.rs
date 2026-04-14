#[derive(Clone, Debug)]
pub struct EntryRunStats {
    pub entry_id: u32,
    pub uncompressed_size: u64,
    pub compressed_size: u64,
    pub stored_effective: bool,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ExplainReport {
    pub archive: ExplainArchive,
    pub files: Vec<ExplainFile>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ExplainArchive {
    pub entropy: f64,
    pub strategy: String,
    pub global_huffman: bool,
    pub predicted_ratio: f64,
    pub actual_ratio: f64,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ExplainFile {
    pub name: String,
    pub size: u64,
    pub entropy: f64,
    pub predicted: String,
    pub actual: String,
    pub predicted_ratio: f64,
    pub actual_ratio: f64,
    pub blocks: Vec<ExplainBlock>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ExplainBlock {
    pub id: u64,
    pub offset: u64,
    pub length: u64,
    pub predicted: String,
    pub actual: String,
    pub predicted_ratio: f64,
    pub actual_ratio: f64,
    pub matches: ExplainMatches,
    pub match_dominance: String,
    pub fallback: bool,
    pub fallback_reason: String,
    pub lz77_tokens: usize,
    pub compressed_bytes: u64,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ExplainMatches {
    pub window: u32,
    pub index: u32,
}
