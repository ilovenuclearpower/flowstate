use thiserror::Error;

#[derive(Debug, Error)]
pub enum FlowstateError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("database error: {0}")]
    Database(String),
}
