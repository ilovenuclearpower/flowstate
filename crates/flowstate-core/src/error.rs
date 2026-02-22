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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flowstate_error_display() {
        assert_eq!(
            FlowstateError::NotFound("task-1".into()).to_string(),
            "not found: task-1"
        );
        assert_eq!(
            FlowstateError::InvalidInput("bad data".into()).to_string(),
            "invalid input: bad data"
        );
        assert_eq!(
            FlowstateError::Database("conn failed".into()).to_string(),
            "database error: conn failed"
        );
    }
}
