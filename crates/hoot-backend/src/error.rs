#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum HootError {
    #[error("database error: {message}")]
    Database { message: String },
    #[error("wrong database password")]
    WrongPassword,
    #[error("keyring error: {message}")]
    Keyring { message: String },
    #[error("invalid nsec: {message}")]
    InvalidNsec { message: String },
    #[error("relay error: {message}")]
    Relay { message: String },
    #[error("nostr error: {message}")]
    Nostr { message: String },
    #[error("json error: {message}")]
    Json { message: String },
    #[error("not found: {entity} {id}")]
    NotFound { entity: String, id: String },
    #[error("internal error: {message}")]
    Internal { message: String },
}

pub type HootResult<T> = std::result::Result<T, HootError>;

impl From<anyhow::Error> for HootError {
    fn from(value: anyhow::Error) -> Self {
        let message = value.to_string();
        if crate::db::format_unlock_error(&value) == "Wrong password" {
            HootError::WrongPassword
        } else {
            HootError::Internal { message }
        }
    }
}

impl From<serde_json::Error> for HootError {
    fn from(value: serde_json::Error) -> Self {
        HootError::Json {
            message: value.to_string(),
        }
    }
}

impl From<RelayError> for HootError {
    fn from(value: RelayError) -> Self {
        HootError::Relay {
            message: value.to_string(),
        }
    }
}

#[derive(Debug)]
pub enum RelayError {
    RelayNotConnected,
    SerdeJson(serde_json::Error),
    Generic(String),
    Empty,
    DecodeFailed,
}

pub use RelayError as Error;

impl From<serde_json::Error> for RelayError {
    fn from(value: serde_json::Error) -> Self {
        Self::SerdeJson(value)
    }
}

impl std::fmt::Display for RelayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RelayError::RelayNotConnected => write!(f, "Relay not connected"),
            RelayError::SerdeJson(err) => write!(f, "JSON serialization error: {}", err),
            RelayError::Generic(s) => write!(f, "{}", s),
            RelayError::Empty => write!(f, "Data was empty"),
            RelayError::DecodeFailed => write!(f, "Could not decode JSON data."),
        }
    }
}

pub type Result<T> = core::result::Result<T, RelayError>;
