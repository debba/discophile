use thiserror::Error;

/// Music and server errors. The UI maps each variant to a localized message (see `l10n::music_error`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MusicError {
    #[error("invalid server")]
    InvalidServer,
    #[error("insecure server")]
    InsecureServer,
    #[error("missing credentials")]
    MissingCredentials,
    #[error("authentication failed")]
    Authentication,
    #[error("forbidden")]
    Forbidden,
    #[error("server unavailable")]
    ServerUnavailable,
    #[error("invalid response")]
    InvalidResponse,
    #[error("unsupported")]
    Unsupported,
    #[error("token authentication unsupported")]
    TokenAuthenticationUnsupported,
    #[error("empty queue")]
    EmptyQueue,
    #[error("unavailable in preview")]
    UnavailableInPreview,
    #[error("not found")]
    NotFound,
    #[error("playlist changed")]
    PlaylistChanged,
}

impl From<reqwest::Error> for MusicError {
    fn from(error: reqwest::Error) -> Self {
        if error.is_decode() { MusicError::InvalidResponse } else { MusicError::ServerUnavailable }
    }
}

impl From<serde_json::Error> for MusicError {
    fn from(_: serde_json::Error) -> Self {
        MusicError::InvalidResponse
    }
}

pub type MusicResult<T> = Result<T, MusicError>;
