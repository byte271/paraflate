use std::path::PathBuf;
use thiserror::Error;

pub type ParaflateResult<T> = Result<T, ParaflateError>;

#[derive(Debug, Error)]
pub enum ParaflateError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid path: {0}")]
    InvalidPath(PathBuf),
    #[error("empty archive")]
    EmptyArchive,
    #[error("entry not found: {0}")]
    EntryNotFound(String),
    #[error("compression failed: {0}")]
    CompressionFailed(String),
    #[error("zip structure: {0}")]
    ZipStructure(String),
    #[error("scheduler shutdown")]
    SchedulerShutdown,
    #[error("worker join failed")]
    WorkerJoin,
    #[error("invariant violated: {0}")]
    InvariantViolated(String),
    #[error("verification failed: {message}")]
    VerificationFailed {
        message: String,
        entry: Option<String>,
    },
    #[error("predictive planning failed: {0}")]
    PredictivePlanning(String),
    #[error("unsupported input: {0}")]
    UnsupportedInput(String),
    #[error("archive consistency: {0}")]
    ArchiveConsistency(String),
}

impl ParaflateError {
    pub fn verification(message: impl Into<String>, entry: Option<String>) -> Self {
        Self::VerificationFailed {
            message: message.into(),
            entry,
        }
    }
}
