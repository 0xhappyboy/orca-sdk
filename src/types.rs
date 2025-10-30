#[derive(Debug)]
pub enum OrcaError {
    Error(String),
    NetworkError(String),
    TransactionError(String),
    ParseError(String),
}

pub type OrcaResult<T> = Result<T, OrcaError>;
