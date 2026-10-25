//! Native sign-in of Discophile for computers: the authorization code grant with PKCE (S256), opened in
//! the system browser, with the answer delivered to a loopback redirect.
//!
//! The app binds `127.0.0.1:0`, creates an [`AuthorizationRequest`] for the port, opens
//! [`AuthorizationRequest::url`], feeds each received request to [`AuthorizationRequest::parse_loopback_request`]
//! and [`AuthorizationRequest::callback`] (answering with [`crate::oauth::loopback_response`]), and abandons
//! the attempt after [`SIGN_IN_TIMEOUT_SECS`]. The code then goes to `ConnectClient::exchange_code`.

use std::fmt;

use url::Url;

use super::discovery::{DESKTOP_CLIENT_ID, ServerInfo};
use super::error::{ConnectError, known_oauth_code};
use super::instance::InstanceOrigin;
use crate::oauth;

pub const CALLBACK_PATH: &str = "/connect/callback";
pub const SCOPE: &str = "account:read offline_access";
/// Sign-in attempts are abandoned after five minutes.
pub const SIGN_IN_TIMEOUT_SECS: u64 = 300;
/// The protocol accepts loopback redirect ports from 1024 to 65535.
pub const MIN_REDIRECT_PORT: u16 = 1024;

/// `http://127.0.0.1:<port>/connect/callback`.
pub fn loopback_redirect(port: u16) -> Url {
    Url::parse(&format!("http://127.0.0.1:{port}{CALLBACK_PATH}")).expect("valid loopback URL")
}

/// The authorization endpoint (which has no query of its own) with this attempt's parameters.
pub fn authorization_url(info: &ServerInfo, redirect: &Url, state: &str, challenge: &str) -> Url {
    let parameters = oauth::form(&[
        ("response_type", "code"),
        ("client_id", DESKTOP_CLIENT_ID),
        ("redirect_uri", redirect.as_str()),
        ("scope", if info.scopes_supported.iter().any(|s| s == "connect:manage") { "account:read offline_access connect:manage" } else { SCOPE }),
        ("state", state),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        ("resource", &info.resource),
    ]);
    let mut url = info.authorization_endpoint.clone();
    url.set_query(Some(&parameters));
    url
}

/// Validates a redirect received on the loopback listener and returns the authorization code.
///
/// [`ConnectError::InvalidCallback`] means the request is not this attempt's answer (wrong address or
/// state, repeated parameters, no code) and the listener keeps waiting. `access_denied` is
/// [`ConnectError::Cancelled`], other errors are [`ConnectError::Rejected`], and an `iss` other than the
/// issuer is [`ConnectError::IssuerMismatch`].
pub fn callback(url: &Url, redirect: &Url, state: &str, issuer: &str) -> Result<String, ConnectError> {
    let invalid = ConnectError::InvalidCallback;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || url.port() != redirect.port()
        || url.path() != CALLBACK_PATH
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(invalid);
    }
    // The redirect's query is form-encoded (RFC 6749, appendix B).
    let query = url.query().ok_or(ConnectError::InvalidCallback)?;
    let items: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes()).into_owned().collect();
    for (index, (name, _)) in items.iter().enumerate() {
        if items[..index].iter().any(|(earlier, _)| earlier == name) {
            return Err(invalid);
        }
    }
    let value = |name: &str| items.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str());
    if value("state") != Some(state) {
        return Err(invalid);
    }
    if let Some(iss) = value("iss")
        && iss != issuer
    {
        return Err(ConnectError::IssuerMismatch);
    }
    match value("error") {
        Some("access_denied") => return Err(ConnectError::Cancelled),
        Some(code) => return Err(ConnectError::Rejected(known_oauth_code(code).to_string())),
        None => {}
    }
    match value("code") {
        Some(code) if !code.is_empty() => Ok(code.to_string()),
        _ => Err(invalid),
    }
}

/// Reads a raw request received on the loopback listener: only `GET /connect/callback?…` with the exact
/// `Host: 127.0.0.1:<port>` header is returned.
pub fn parse_loopback_request(raw: &[u8], redirect: &Url) -> Option<Url> {
    oauth::parse_loopback_request(raw, redirect, CALLBACK_PATH)
}

/// One sign-in attempt: its random values, redirect and the page to open.
#[derive(Clone, PartialEq, Eq)]
pub struct AuthorizationRequest {
    pub origin: InstanceOrigin,
    pub issuer: String,
    /// The `resource` of the authorization request, repeated in the token request.
    pub resource: String,
    pub redirect: Url,
    pub state: String,
    pub verifier: String,
    /// `base64url(SHA-256(verifier))`.
    pub challenge: String,
    /// The authorization page to open in the browser.
    pub url: Url,
}

impl fmt::Debug for AuthorizationRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthorizationRequest").field("origin", &self.origin).field("redirect", &self.redirect.as_str()).finish_non_exhaustive()
    }
}

impl AuthorizationRequest {
    /// A new attempt for a loopback listener on `port`, with a fresh verifier and state from the system's
    /// secure random generator.
    pub fn new(info: &ServerInfo, port: u16) -> Result<Self, ConnectError> {
        let verifier = oauth::random_token().ok_or(ConnectError::Unavailable)?;
        let state = oauth::random_token().ok_or(ConnectError::Unavailable)?;
        Self::with_values(info, port, verifier, state)
    }

    /// An attempt with the given verifier and state.
    pub fn with_values(info: &ServerInfo, port: u16, verifier: String, state: String) -> Result<Self, ConnectError> {
        info.check_desktop_sign_in()?;
        if port < MIN_REDIRECT_PORT {
            return Err(ConnectError::Unavailable);
        }
        let redirect = loopback_redirect(port);
        let challenge = oauth::challenge(&verifier);
        let url = authorization_url(info, &redirect, &state, &challenge);
        Ok(Self {
            origin: info.origin.clone(),
            issuer: info.issuer.clone(),
            resource: info.resource.clone(),
            redirect,
            state,
            verifier,
            challenge,
            url,
        })
    }

    /// See [`callback`].
    pub fn callback(&self, url: &Url) -> Result<String, ConnectError> {
        callback(url, &self.redirect, &self.state, &self.issuer)
    }

    /// See [`parse_loopback_request`].
    pub fn parse_loopback_request(&self, raw: &[u8]) -> Option<Url> {
        parse_loopback_request(raw, &self.redirect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect::discovery::tests::document;

    fn info() -> ServerInfo {
        let origin = InstanceOrigin::official();
        ServerInfo::parse(&origin, document(origin.as_str()).to_string().as_bytes()).unwrap()
    }

    fn request() -> AuthorizationRequest {
        // RFC 7636 appendix B verifier.
        AuthorizationRequest::with_values(&info(), 53682, "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".into(), "expected".into())
            .unwrap()
    }

    #[test]
    fn authorization_url_carries_pkce_state_scope_and_resource() {
        let request = request();
        assert_eq!(request.challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        assert_eq!(request.redirect.as_str(), "http://127.0.0.1:53682/connect/callback");
        let url = &request.url;
        assert_eq!(url.origin().ascii_serialization(), "https://connect.discophile.app");
        assert_eq!(url.path(), "/oauth/authorize");
        let value = |name: &str| {
            let values: Vec<String> = url.query_pairs().filter(|(k, _)| k == name).map(|(_, v)| v.into_owned()).collect();
            assert!(values.len() <= 1, "{name} repeated");
            values.into_iter().next()
        };
        assert_eq!(value("response_type").as_deref(), Some("code"));
        assert_eq!(value("client_id").as_deref(), Some("discophile-desktop"));
        assert_eq!(value("redirect_uri").as_deref(), Some("http://127.0.0.1:53682/connect/callback"));
        assert_eq!(value("scope").as_deref(), Some("account:read offline_access"));
        assert_eq!(value("state").as_deref(), Some("expected"));
        assert_eq!(value("code_challenge").as_deref(), Some("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"));
        assert_eq!(value("code_challenge_method").as_deref(), Some("S256"));
        assert_eq!(value("resource").as_deref(), Some("https://connect.discophile.app/api/v1"));
        assert!(value("client_secret").is_none());
        let query = url.query().unwrap();
        assert!(query.contains("scope=account%3Aread%20offline_access"));
        assert!(query.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A53682%2Fconnect%2Fcallback"));
        assert!(!format!("{request:?}").contains("dBjftJeZ"));
    }

    #[test]
    fn new_attempts_use_fresh_random_values_and_valid_ports() {
        let first = AuthorizationRequest::new(&info(), 50_000).unwrap();
        let second = AuthorizationRequest::new(&info(), 50_000).unwrap();
        assert_eq!(first.verifier.len(), 43);
        assert_eq!(first.state.len(), 43);
        assert_eq!(oauth::decode_base64_url(&first.verifier).unwrap().len(), 32);
        assert_eq!(first.challenge, oauth::challenge(&first.verifier));
        assert_ne!(first.state, second.state);
        assert_ne!(first.verifier, first.state);
        assert_eq!(AuthorizationRequest::new(&info(), 80).unwrap_err(), ConnectError::Unavailable);
        let mut ios_only = info();
        ios_only.native_clients = vec!["discophile-ios".into()];
        assert_eq!(AuthorizationRequest::new(&ios_only, 50_000).unwrap_err(), ConnectError::NotOffered);
    }

    #[test]
    fn callback_accepts_only_this_attempts_answer() {
        let request = request();
        let callback = |suffix: &str| request.callback(&Url::parse(&format!("http://127.0.0.1:53682{suffix}")).unwrap());
        assert_eq!(callback("/connect/callback?code=abc&state=expected").unwrap(), "abc");
        assert_eq!(callback("/connect/callback?code=a%2Bb&state=expected&iss=https%3A%2F%2Fconnect.discophile.app").unwrap(), "a+b");
        assert_eq!(callback("/connect/callback?code=a+b&state=expected").unwrap(), "a b");
        for query in [
            "?code=abc&state=wrong",
            "?code=abc",
            "?state=expected",
            "?state=expected&code=",
            "?code=abc&state=expected&state=expected",
            "?code=a&code=b&state=expected",
            "?code=abc&state=expected&extra=1&extra=2",
        ] {
            assert_eq!(callback(&format!("/connect/callback{query}")).unwrap_err(), ConnectError::InvalidCallback, "{query}");
        }
        assert_eq!(callback("/auth/callback?code=abc&state=expected").unwrap_err(), ConnectError::InvalidCallback);
        assert_eq!(callback("/connect/callback?code=abc&state=expected#x").unwrap_err(), ConnectError::InvalidCallback);
        for other in [
            "http://localhost:53682/connect/callback?code=abc&state=expected",
            "http://127.0.0.1:53683/connect/callback?code=abc&state=expected",
            "https://127.0.0.1:53682/connect/callback?code=abc&state=expected",
        ] {
            assert_eq!(request.callback(&Url::parse(other).unwrap()).unwrap_err(), ConnectError::InvalidCallback, "{other}");
        }
        assert_eq!(callback("/connect/callback?error=access_denied&state=expected").unwrap_err(), ConnectError::Cancelled);
        assert_eq!(
            callback("/connect/callback?error=access_denied&error_description=The+person+denied+access.&state=expected&iss=https%3A%2F%2Fconnect.discophile.app")
                .unwrap_err(),
            ConnectError::Cancelled
        );
        assert_eq!(
            callback("/connect/callback?error=invalid_target&state=expected").unwrap_err(),
            ConnectError::Rejected("invalid_target".into())
        );
        assert_eq!(callback("/connect/callback?error=%3Cscript%3E&state=expected").unwrap_err(), ConnectError::Rejected("error".into()));
        // An error with the wrong state is not this attempt's answer.
        assert_eq!(callback("/connect/callback?error=access_denied&state=wrong").unwrap_err(), ConnectError::InvalidCallback);
        assert_eq!(
            callback("/connect/callback?code=abc&state=expected&iss=https%3A%2F%2Fevil.example").unwrap_err(),
            ConnectError::IssuerMismatch
        );
    }

    #[test]
    fn loopback_accepts_only_the_exact_callback_request() {
        let request = request();
        let raw = |line: &str, host: &str| format!("{line}\r\nHost: {host}\r\nAccept: */*\r\n\r\n").into_bytes();
        let url = request.parse_loopback_request(&raw("GET /connect/callback?code=c&state=s HTTP/1.1", "127.0.0.1:53682")).unwrap();
        assert_eq!(url.as_str(), "http://127.0.0.1:53682/connect/callback?code=c&state=s");
        for (line, host) in [
            ("POST /connect/callback?code=c HTTP/1.1", "127.0.0.1:53682"),
            ("GET /auth/callback?code=c HTTP/1.1", "127.0.0.1:53682"),
            ("GET /connect/callbackx?code=c HTTP/1.1", "127.0.0.1:53682"),
            ("GET /connect/callback HTTP/1.1", "127.0.0.1:53682"),
            ("GET /connect/callback?code=c HTTP/1.1", "localhost:53682"),
            ("GET /connect/callback?code=c HTTP/1.1", "127.0.0.1:1"),
        ] {
            assert!(request.parse_loopback_request(&raw(line, host)).is_none(), "{line} {host}");
        }
        assert!(request.parse_loopback_request(b"GET /connect/callback?code=c HTTP/1.1\r\nHost: 127.0.0.1:53682\r\n").is_none());
        let two_hosts = b"GET /connect/callback?code=c HTTP/1.1\r\nHost: 127.0.0.1:53682\r\nHost: 127.0.0.1:53682\r\n\r\n";
        assert!(request.parse_loopback_request(two_hosts).is_none());
    }
}
