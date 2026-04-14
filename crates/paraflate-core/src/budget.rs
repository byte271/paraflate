#[derive(Clone, Copy, Debug)]
pub struct MemoryBudget {
    pub pool_bytes: usize,
    pub max_in_flight_bytes: usize,
    pub read_chunk_bytes: usize,
}

impl Default for MemoryBudget {
    fn default() -> Self {
        Self {
            pool_bytes: 64 * 1024 * 1024,
            max_in_flight_bytes: 256 * 1024 * 1024,
            read_chunk_bytes: 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ExecutionBudget {
    pub worker_threads: usize,
    pub pipeline_depth: usize,
    pub io_lane_count: usize,
    pub max_pending_tasks: usize,
    pub memory: MemoryBudget,
}

impl Default for ExecutionBudget {
    fn default() -> Self {
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().max(2))
            .unwrap_or(4);
        Self {
            worker_threads: threads,
            pipeline_depth: threads.saturating_mul(2).max(8),
            io_lane_count: threads.min(8).max(2),
            max_pending_tasks: threads.saturating_mul(32).max(64),
            memory: MemoryBudget::default(),
        }
    }
}
