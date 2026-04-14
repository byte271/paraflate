use thiserror::Error;

#[derive(Debug, Error)]
pub enum HarnessError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("paraflate: {0}")]
    Paraflate(#[from] paraflate_core::ParaflateError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("zip: {0}")]
    Zip(String),
    #[error("validation: {0}")]
    Validation(String),
    #[error("{0}")]
    Other(String),
}

pub type HarnessResult<T> = Result<T, HarnessError>;
