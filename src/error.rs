use std::io;

use thiserror::Error;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("command failed: {0}")]
    CommandFailed(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("invalid config: {0}")]
    InvalidConfig(String),
}
