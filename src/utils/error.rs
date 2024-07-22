use thiserror::Error;

#[derive(Debug, Error)]
pub enum CustomError {
    #[error("Command execution failed: {0}")]
    CommandExecutionFailed(String),
    #[error("Unexpected error: {0}")]
    UnexpectedError(String),
}

impl From<std::io::Error> for CustomError {
    fn from(err: std::io::Error) -> Self {
        CustomError::CommandExecutionFailed(err.to_string())
    }
}
