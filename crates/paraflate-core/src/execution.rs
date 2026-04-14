#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ExecutionPhase {
    #[default]
    Discovery,
    MetadataScan,
    FileRead,
    Sampling,
    GlobalAnalysis,
    ChunkPlanning,
    BlockScheduling,
    Compression,
    Encoding,
    ZipWriting,
    Finalization,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SchedulerPolicy {
    Balanced,
    ThroughputBiased,
    RatioBiased,
    MemoryConstrained,
}

#[derive(Clone, Debug)]
pub struct ExecutionPolicy {
    pub scheduler: SchedulerPolicy,
    pub sample_rate: f64,
    pub min_sample_bytes: usize,
    pub max_sample_bytes_per_entry: usize,
    pub large_file_bytes: usize,
    pub base_block_bytes: usize,
    pub max_block_bytes: usize,
    pub min_block_bytes: usize,
    pub adaptive_block_feedback: bool,
}

impl Default for ExecutionPolicy {
    fn default() -> Self {
        Self {
            scheduler: SchedulerPolicy::Balanced,
            sample_rate: 0.05,
            min_sample_bytes: 4096,
            max_sample_bytes_per_entry: 256 * 1024,
            large_file_bytes: 8 * 1024 * 1024,
            base_block_bytes: 256 * 1024,
            max_block_bytes: 4 * 1024 * 1024,
            min_block_bytes: 32 * 1024,
            adaptive_block_feedback: false,
        }
    }
}
