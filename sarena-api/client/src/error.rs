#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("HTTP error: {0}")]
    Http(#[from] http::Error),
    #[error("invalid URI: {0}")]
    InvalidUri(#[from] http::uri::InvalidUri),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Invalid host: {0}")]
    InvalidHost(String),
    #[error("Invalid scheme: {0}")]
    InvalidScheme(String),
    #[error("Resource {0} does not exist")]
    ResourceNotExist(String),
    #[error("request to {endpoint} failed with status {status}: {body}")]
    UnexpectedStatus {
        endpoint: String,
        status: http::StatusCode,
        body: String,
    },
    #[error("Timeout")]
    Timeout,
}

impl From<hyper_util::client::legacy::Error> for TransportError {
    fn from(e: hyper_util::client::legacy::Error) -> Self {
        Self::NetworkError(e.to_string())
    }
}

impl From<hyper::Error> for TransportError {
    fn from(e: hyper::Error) -> Self {
        Self::NetworkError(e.to_string())
    }
}

pub type Res<T> = Result<T, TransportError>;
