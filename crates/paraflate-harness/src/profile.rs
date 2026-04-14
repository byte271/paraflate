use paraflate_core::{
    ArchiveLayout, ArchiveProfile, CompressionMethod, CompressionProfile, DeflateStrategy,
    ExecutionBudget, ExecutionPolicy, PlanningAggression, PredictiveMode, PredictiveRuntimeConfig,
    SchedulerPolicy, VerificationMode,
};

pub fn build_profile(
    level: u32,
    threads: usize,
    prediction: PredictiveMode,
    verification: VerificationMode,
    adaptive: bool,
    global_huffman: bool,
    scheduler: SchedulerPolicy,
    planning: PlanningAggression,
) -> ArchiveProfile {
    let mut budget = ExecutionBudget::default();
    if threads > 0 {
        budget.worker_threads = threads;
        budget.pipeline_depth = threads.saturating_mul(2).max(8);
        budget.io_lane_count = threads.min(8).max(2);
        budget.max_pending_tasks = threads.saturating_mul(32).max(64);
    }
    let mut execution = ExecutionPolicy::default();
    execution.scheduler = scheduler;
    execution.adaptive_block_feedback = adaptive;
    let compression = CompressionProfile {
        method: CompressionMethod::Deflate,
        level,
        strategy: DeflateStrategy::Default,
        window_bits: 15,
        global_huffman,
    };
    ArchiveProfile {
        layout: ArchiveLayout::DeterministicLexical,
        compression,
        execution,
        budget,
        reproducible: true,
        predictive: PredictiveRuntimeConfig {
            mode: prediction,
            verification,
            planning,
        },
    }
}
