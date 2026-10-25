use thiserror::Error;

use super::instance::InstanceUrlError;

/// Discophile Connect failures. The app maps each variant to a localized message; none carries text from
/// the server except a known OAuth error code.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConnectError {
    #[error("invalid instance address: {0}")]
    Address(#[from] InstanceUrlError),
    /// The instance could not be reached (connection, TLS or timeout).
    #[error("network error")]
    Network,
    /// The server answered with a redirect, which is never followed.
    #[error("redirect refused")]
    Redirected,
    /// Malformed, oversized or unexpected response.
    #[error("invalid response")]
    InvalidResponse,
    /// Not a Discophile Connect instance, or no protocol version this client implements.
    #[error("unsupported protocol")]
    Unsupported,
    /// The discovery document describes another origin, or a URL points to another origin.
    #[error("origin mismatch")]
    OriginMismatch,
    /// The instance identifier changed, or the account belongs to another instance.
    #[error("instance mismatch")]
    InstanceMismatch,
    /// The instance does not offer what is needed: the `account` capability, the desktop client or its scopes.
    #[error("not offered by this instance")]
    NotOffered,
    /// `invalid_grant`: the delegation ended. Tokens must be cleared and the person must sign in again.
    #[error("sign-in required")]
    SignInRequired,
    /// A refresh answered without a new refresh token. The previous one is never presented again (that
    /// would revoke the delegation), so the new access token was revoked and the person must sign in again.
    #[error("refresh token not rotated")]
    RefreshNotRotated,
    /// `401` from the API: the access token is missing, expired or revoked.
    #[error("access token rejected")]
    Unauthorized,
    /// `403` from the API.
    #[error("insufficient scope")]
    InsufficientScope,
    #[error("rate limited")]
    RateLimited { retry_after: Option<u64> },
    /// `5xx`, `server_error` or `temporarily_unavailable`. Tokens are kept.
    #[error("server unavailable ({0})")]
    ServerUnavailable(u16),
    /// Another OAuth error, by its RFC 6749 code (for example `invalid_client` or `invalid_target`).
    #[error("request rejected: {0}")]
    Rejected(String),
    /// The person declined (`access_denied`).
    #[error("cancelled")]
    Cancelled,
    /// The callback's `iss` differs from the instance's issuer.
    #[error("issuer mismatch")]
    IssuerMismatch,
    /// A loopback request that is not this attempt's callback; keep waiting.
    #[error("invalid callback")]
    InvalidCallback,
    /// A local failure, such as the random generator or an unusable loopback port.
    #[error("unavailable")]
    Unavailable,
}

impl ConnectError {
    /// Whether stored tokens must be deleted: only when the delegation is known to have ended. Network
    /// errors, `5xx` and other errors keep them for a later attempt.
    pub fn ends_delegation(&self) -> bool {
        matches!(self, Self::SignInRequired | Self::RefreshNotRotated)
    }
}

/// OAuth error codes defined by version 1. Anything else is reported as `error`.
pub(crate) fn known_oauth_code(code: &str) -> &'static str {
    const CODES: [&str; 10] = [
        "invalid_request",
        "invalid_client",
        "invalid_grant",
        "unauthorized_client",
        "unsupported_grant_type",
        "invalid_scope",
        "invalid_target",
        "access_denied",
        "temporarily_unavailable",
        "server_error",
    ];
    CODES.iter().find(|c| **c == code).copied().unwrap_or("error")
}
