use paraflate_core::{
    DeflateBlockDebugRecord, EntryRunStats, ExecutionPhase, PredictiveMode, VerificationMode,
};
use paraflate_zip::ZipFinalizeSummary;

use crate::verification::VerificationReport;

#[derive(Clone, Debug)]
pub struct RunReport {
    pub phase: ExecutionPhase,
    pub entries: u64,
    pub uncompressed_bytes: u64,
    pub compressed_bytes: u64,
    pub worker_threads: usize,
    pub zip: Option<ZipFinalizeSummary>,
    pub verification: Option<VerificationReport>,
    pub elapsed_ms: u128,
    pub predictive_mode: PredictiveMode,
    pub verification_mode: VerificationMode,
    pub debug_blocks: Option<Vec<DeflateBlockDebugRecord>>,
    pub entry_stats: Option<Vec<EntryRunStats>>,
}
