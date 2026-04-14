use paraflate_core::{CompressionMethod, CompressionProfile, DeflateStrategy, ParaflateError};

pub fn merge_profile_stored(base: &CompressionProfile) -> CompressionProfile {
    let mut p = base.clone();
    p.method = CompressionMethod::Stored;
    p
}

pub fn merge_profile_fixed_deflate(base: &CompressionProfile) -> CompressionProfile {
    let mut p = base.clone();
    p.method = CompressionMethod::Deflate;
    p.strategy = DeflateStrategy::Fixed;
    p.global_huffman = false;
    p
}

pub fn should_retry_compression(err: &ParaflateError) -> bool {
    matches!(
        err,
        ParaflateError::CompressionFailed(_)
            | ParaflateError::InvariantViolated(_)
            | ParaflateError::SchedulerShutdown
            | ParaflateError::WorkerJoin
    )
}
