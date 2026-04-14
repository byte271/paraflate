mod graph;
mod pool;

pub use graph::TaskGraphBuilder;
pub use pool::{CompressionWork, PoolOutcome, WorkerPool, WorkerPoolConfig};
