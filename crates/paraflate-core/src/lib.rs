mod archive;
mod blocks;
mod budget;
mod compression;
mod descriptor;
mod error;
mod execution;
mod explain;
mod ids;
mod predictive;
mod stats;
mod tasks;

pub use archive::{ArchiveLayout, ArchiveProfile};
pub use blocks::{BlockDescriptor, BlockSpan, ChunkPlan};
pub use budget::{ExecutionBudget, MemoryBudget};
pub use compression::{
    CompressionMethod, CompressionProfile, DeflateBlockDebugRecord, DeflateStrategy,
};
pub use descriptor::ArchiveEntryDescriptor;
pub use error::{ParaflateError, ParaflateResult};
pub use execution::{ExecutionPhase, ExecutionPolicy, SchedulerPolicy};
pub use explain::{
    EntryRunStats, ExplainArchive, ExplainBlock, ExplainFile, ExplainMatches, ExplainReport,
};
pub use ids::{BlockId, EntryId, TaskId};
pub use predictive::{
    EntryCompressionPlan, PlanningAggression, PredictedDeflatePath, PredictiveArchivePlan,
    PredictiveMode, PredictiveRuntimeConfig, VerificationMode,
};
pub use stats::{ByteClassHistogram, GlobalStatisticsSummary, PatternDigest};
pub use tasks::{CompressionTaskKind, TaskGraph, TaskNode};
