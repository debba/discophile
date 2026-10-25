//! Requests to a Connect instance: discovery, the token endpoint (code exchange and refresh), revocation and
//! the account. Redirects are never followed, bearer tokens stay on the instance's origin, and response
//! bodies are bounded. External HTTPS profile pictures are fetched without credentials.
//!
//! Refresh tokens rotate: the caller refreshes at most once at a time and stores the credential returned
//! by [`ConnectClient::refresh`] before using it. [`ConnectError::SignInRequired`] means the delegation ended
//! and the tokens must be deleted; network errors and `5xx` keep them.

use futures::StreamExt;
use reqwest::header::{ACCEPT, CONTENT_TYPE, RETRY_AFTER};
use serde::Deserialize;
use url::Url;

use super::account::AccountInfo;
use super::authorization::{AuthorizationRequest, SCOPE};
use super::credential::{ConnectCredential, TokenSet};
use super::discovery::{ACCOUNT_CAPABILITY, DESKTOP_CLIENT_ID, SERVER_INFO_PATH, ServerInfo};
use super::error::{ConnectError, known_oauth_code};
use super::instance::InstanceOrigin;
use crate::oauth;

/// Largest accepted response body.
pub const MAX_RESPONSE_BYTES: usize = 262_144;
pub const MAX_AVATAR_BYTES: usize = 524_288;
/// Longest accepted access token lifetime; longer values are shortened.
const MAX_EXPIRES_IN_SECS: f64 = 366.0 * 24.0 * 3600.0;

struct Reply {
    status: u16,
    body: Vec<u8>,
    retry_after: Option<u64>,
}

impl Reply {
    fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    fn error_code(&self) -> Option<String> {
        let value: serde_json::Value = serde_json::from_slice(&self.body).ok()?;
        value.get("error")?.as_str().map(str::to_string)
    }

    /// Token and revocation endpoint errors (RFC 6749, section 5.2).
    fn oauth_failure(&self) -> ConnectError {
        match self.status {
            429 => return ConnectError::RateLimited { retry_after: self.retry_after },
            500..=599 => return ConnectError::ServerUnavailable(self.status),
            _ => {}
        }
        match self.error_code().as_deref() {
            Some("invalid_grant") => ConnectError::SignInRequired,
            Some("access_denied") => ConnectError::Cancelled,
            Some("temporarily_unavailable" | "server_error") => ConnectError::ServerUnavailable(self.status),
            Some(code) if known_oauth_code(code) != "error" => ConnectError::Rejected(code.to_string()),
            _ => ConnectError::InvalidResponse,
        }
    }

    /// `/api/v1` errors.
    fn api_failure(&self) -> ConnectError {
        match self.status {
            401 => ConnectError::Unauthorized,
            403 => ConnectError::InsufficientScope,
            404 => ConnectError::NotOffered,
            429 => ConnectError::RateLimited { retry_after: self.retry_after },
            500..=599 => ConnectError::ServerUnavailable(self.status),
            _ => ConnectError::InvalidResponse,
        }
    }
}

/// A validated token response; the refresh token is checked by the caller.
struct Issued {
    access_token: String,
    refresh_token: Option<String>,
    scopes: Vec<String>,
    expires_at: i64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    token_type: String,
    expires_in: f64,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

/// The HTTP client of one app. It holds no state: pass the instance's [`ServerInfo`] to every call.
#[derive(Debug, Clone)]
pub struct ConnectClient {
    http: reqwest::Client,
}

impl Default for ConnectClient {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectClient {
    /// Uses [`crate::http::server_client`]: no redirects, bounded timeouts.
    pub fn new() -> Self {
        Self::with_http(crate::http::server_client())
    }

    pub fn with_http(http: reqwest::Client) -> Self {
        Self { http }
    }

    async fn send(
        &self,
        request: reqwest::RequestBuilder,
        url: &Url,
    ) -> Result<Reply, ConnectError> {
        self.send_bounded(request.header(ACCEPT, "application/json"), url, MAX_RESPONSE_BYTES).await
    }

    async fn send_bounded(
        &self,
        request: reqwest::RequestBuilder,
        url: &Url,
        limit: usize,
    ) -> Result<Reply, ConnectError> {
        let response = request
            .send()
            .await
            .map_err(|_| ConnectError::Network)?;
        let status = response.status().as_u16();
        // Browsers follow redirects themselves; a different final URL means one happened.
        if (300..400).contains(&status) || response.url() != url {
            return Err(ConnectError::Redirected);
        }
        let retry_after = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse().ok());
        if response
            .content_length()
            .is_some_and(|length| length > limit as u64)
        {
            return Err(ConnectError::InvalidResponse);
        }
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| ConnectError::Network)?;
            if body.len() + chunk.len() > limit {
                return Err(ConnectError::InvalidResponse);
            }
            body.extend_from_slice(&chunk);
        }
        Ok(Reply {
            status,
            body,
            retry_after,
        })
    }

    pub async fn catalog(
        &self,
        info: &ServerInfo,
    ) -> Result<super::services::Catalog, ConnectError> {
        if !info.is_consistent() {
            return Err(ConnectError::OriginMismatch);
        }
        let url = info.origin.url("/api/v1/plans");
        let reply = self.send(self.http.get(url.clone()), &url).await?;
        if !reply.ok() {
            return Err(reply.api_failure());
        }
        let catalog: super::services::Catalog =
            serde_json::from_slice(&reply.body).map_err(|_| ConnectError::InvalidResponse)?;
        if catalog.currency != "EUR"
            || catalog.activation != "email"
            || catalog.tiers.len() > 20
            || catalog.contact.len() > 254
            || catalog.contact.contains(['\r', '\n', '?', '&'])
        {
            return Err(ConnectError::InvalidResponse);
        }
        Ok(catalog)
    }

    pub async fn service(
        &self,
        info: &ServerInfo,
        credential: &ConnectCredential,
        operation: &serde_json::Value,
    ) -> Result<serde_json::Value, ConnectError> {
        check_binding(info, credential)?;
        if !info.is_consistent() {
            return Err(ConnectError::OriginMismatch);
        }
        if !info.capability_usable("devices") {
            return Err(ConnectError::NotOffered);
        }
        let url = info.origin.url("/api/v1/connect");
        let body = serde_json::to_vec(operation).map_err(|_| ConnectError::InvalidResponse)?;
        if body.len() > 524_288 {
            return Err(ConnectError::Unavailable);
        }
        let reply = self
            .send_bounded(
                self.http
                    .post(url.clone())
                    .bearer_auth(&credential.access_token)
                    .header(ACCEPT, "application/json")
                    .header(CONTENT_TYPE, "application/json")
                    .body(body),
                &url,
                4_194_304,
            )
            .await?;
        if !reply.ok() {
            if matches!(reply.status, 400 | 403 | 404 | 409) {
                let code = reply.error_code().unwrap_or_default();
                if [
                    "forbidden",
                    "invalid_input",
                    "revision_conflict",
                    "not_found",
                    "operation_conflict",
                ]
                .contains(&code.as_str())
                {
                    return Err(ConnectError::Service(code));
                }
            }
            return Err(reply.api_failure());
        }
        serde_json::from_slice(&reply.body).map_err(|_| ConnectError::InvalidResponse)
    }

    async fn post_form(&self, info: &ServerInfo, url: &Url, fields: &[(&str, &str)]) -> Result<Reply, ConnectError> {
        if !info.origin.owns(url) {
            return Err(ConnectError::OriginMismatch);
        }
        let request = self.http.post(url.clone()).header(CONTENT_TYPE, "application/x-www-form-urlencoded").body(oauth::form(fields));
        self.send(request, url).await
    }

    /// Reads and validates the discovery document of `origin`.
    pub async fn server_info(&self, origin: &InstanceOrigin) -> Result<ServerInfo, ConnectError> {
        let url = origin.url(SERVER_INFO_PATH);
        let reply = self.send(self.http.get(url.clone()), &url).await?;
        match reply.status {
            200 => ServerInfo::parse(origin, &reply.body),
            404 => Err(ConnectError::Unsupported),
            _ => Err(reply.api_failure()),
        }
    }

    async fn token(&self, info: &ServerInfo, fields: &[(&str, &str)]) -> Result<Issued, ConnectError> {
        let reply = self.post_form(info, &info.token_endpoint, fields).await?;
        if !reply.ok() {
            return Err(reply.oauth_failure());
        }
        let tokens: TokenResponse = serde_json::from_slice(&reply.body).map_err(|_| ConnectError::InvalidResponse)?;
        if !tokens.token_type.eq_ignore_ascii_case("Bearer")
            || tokens.access_token.is_empty()
            || !tokens.expires_in.is_finite()
            || tokens.expires_in <= 0.0
        {
            return Err(ConnectError::InvalidResponse);
        }
        let scope = tokens.scope.unwrap_or_else(|| SCOPE.to_string());
        Ok(Issued {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token.filter(|t| !t.is_empty()),
            scopes: scope.split(' ').filter(|s| !s.is_empty()).map(str::to_string).collect(),
            expires_at: crate::now_millis() + (tokens.expires_in.min(MAX_EXPIRES_IN_SECS) * 1_000.0) as i64,
        })
    }

    /// Requires a refresh token. Without one, the new access token is revoked (best effort), which ends the
    /// delegation, and [`ConnectError::RefreshNotRotated`] is returned.
    async fn with_refresh_token(&self, info: &ServerInfo, issued: Issued, client_id: &str) -> Result<TokenSet, ConnectError> {
        match issued.refresh_token {
            Some(refresh_token) => {
                Ok(TokenSet { access_token: issued.access_token, refresh_token, scopes: issued.scopes, expires_at: issued.expires_at })
            }
            None => {
                let _ = self.revoke_with_hint(info, &issued.access_token, "access_token", client_id).await;
                Err(ConnectError::RefreshNotRotated)
            }
        }
    }

    /// Exchanges the code from [`AuthorizationRequest::callback`] for tokens.
    pub async fn exchange_code(&self, info: &ServerInfo, request: &AuthorizationRequest, code: &str) -> Result<TokenSet, ConnectError> {
        if request.origin != info.origin || request.issuer != info.issuer {
            return Err(ConnectError::OriginMismatch);
        }
        let issued = self
            .token(
                info,
                &[
                    ("grant_type", "authorization_code"),
                    ("code", code),
                    ("redirect_uri", request.redirect.as_str()),
                    ("client_id", DESKTOP_CLIENT_ID),
                    ("code_verifier", &request.verifier),
                    ("resource", &request.resource),
                ],
            )
            .await?;
        self.with_refresh_token(info, issued, DESKTOP_CLIENT_ID).await.map_err(|_| ConnectError::InvalidResponse)
    }

    /// Rotates the tokens. Store the returned credential before using it: the previous refresh token
    /// stops working. A response without a new refresh token is [`ConnectError::RefreshNotRotated`].
    pub async fn refresh(&self, info: &ServerInfo, credential: &ConnectCredential) -> Result<ConnectCredential, ConnectError> {
        check_binding(info, credential)?;
        let issued = self
            .token(
                info,
                &[
                    ("grant_type", "refresh_token"),
                    ("refresh_token", &credential.refresh_token),
                    ("client_id", &credential.client_id),
                    ("resource", &info.resource),
                ],
            )
            .await?;
        let tokens = self.with_refresh_token(info, issued, &credential.client_id).await?;
        Ok(credential.rotated(tokens))
    }

    /// Revokes the credential's delegation through its refresh token. Only the origin must match, so a
    /// credential can still be revoked after the instance identifier changed.
    pub async fn revoke(&self, info: &ServerInfo, credential: &ConnectCredential) -> Result<(), ConnectError> {
        if credential.origin != info.origin {
            return Err(ConnectError::OriginMismatch);
        }
        self.revoke_token(info, &credential.refresh_token, &credential.client_id).await
    }

    /// Revokes a refresh token issued by this instance to `client_id`.
    pub async fn revoke_token(&self, info: &ServerInfo, refresh_token: &str, client_id: &str) -> Result<(), ConnectError> {
        self.revoke_with_hint(info, refresh_token, "refresh_token", client_id).await
    }

    async fn revoke_with_hint(&self, info: &ServerInfo, token: &str, hint: &str, client_id: &str) -> Result<(), ConnectError> {
        let fields = [("token", token), ("token_type_hint", hint), ("client_id", client_id)];
        let reply = self.post_form(info, &info.revocation_endpoint, &fields).await?;
        if reply.ok() { Ok(()) } else { Err(reply.oauth_failure()) }
    }

    /// `GET /api/v1/account` with an access token issued by this instance.
    pub async fn account(&self, info: &ServerInfo, access_token: &str) -> Result<AccountInfo, ConnectError> {
        if !info.capability_usable(ACCOUNT_CAPABILITY) {
            return Err(ConnectError::NotOffered);
        }
        let url = info.account_url();
        let reply = self.send(self.http.get(url.clone()).bearer_auth(access_token), &url).await?;
        if !reply.ok() {
            return Err(reply.api_failure());
        }
        AccountInfo::parse(info, &reply.body)
    }

    /// The account of a stored credential, which must belong to this instance and account.
    pub async fn account_for(&self, info: &ServerInfo, credential: &ConnectCredential) -> Result<AccountInfo, ConnectError> {
        check_binding(info, credential)?;
        let account = self.account(info, &credential.access_token).await?;
        if account.account.id != credential.account_id {
            return Err(ConnectError::InstanceMismatch);
        }
        Ok(account)
    }

    /// Profile pictures never receive a bearer token outside the Connect instance, and never follow redirects.
    pub async fn avatar(&self, info: &ServerInfo, credential: &ConnectCredential, url: &Url) -> Result<Vec<u8>, ConnectError> {
        check_binding(info, credential)?;
        if !info.is_consistent() || !url.username().is_empty() || url.password().is_some() || url.fragment().is_some()
            || (url.scheme() != "https" && !info.origin.owns(url)) {
            return Err(ConnectError::OriginMismatch);
        }
        let request = self.avatar_request(info, credential, url);
        let reply = self.send_bounded(request, url, MAX_AVATAR_BYTES).await?;
        if !reply.ok() {
            return Err(reply.api_failure());
        }
        Ok(reply.body)
    }

    fn avatar_request(&self, info: &ServerInfo, credential: &ConnectCredential, url: &Url) -> reqwest::RequestBuilder {
        let request = self.http.get(url.clone()).header(ACCEPT, "image/png, image/jpeg, image/webp");
        if info.origin.owns(url) { request.bearer_auth(&credential.access_token) } else { request }
    }
}

/// A credential is used only with the instance (origin and identifier) that issued it.
pub fn check_binding(info: &ServerInfo, credential: &ConnectCredential) -> Result<(), ConnectError> {
    if credential.origin != info.origin {
        return Err(ConnectError::OriginMismatch);
    }
    if credential.instance_id != info.instance_id {
        return Err(ConnectError::InstanceMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::test_server::TestServer;
    use crate::connect::account::tests::example;
    use crate::connect::discovery::tests::document;
    use serde_json::json;

    const TOKENS: &str = r#"{"access_token":"dca_new","token_type":"Bearer","expires_in":3600,"refresh_token":"dcr_new","scope":"account:read offline_access"}"#;

    async fn instance() -> (TestServer, InstanceOrigin, ServerInfo) {
        let server = TestServer::start(|_| (500, String::new())).await;
        let origin = InstanceOrigin::parse(server.base.as_str()).unwrap();
        let info = ServerInfo::parse(&origin, document(origin.as_str()).to_string().as_bytes()).unwrap();
        (server, origin, info)
    }

    fn credential(info: &ServerInfo) -> ConnectCredential {
        let tokens = TokenSet { access_token: "dca_old".into(), refresh_token: "dcr_old+/=".into(), scopes: vec![], expires_at: 0 };
        ConnectCredential::new(info.origin.clone(), info.instance_id.clone(), "u_6sQb2kL8wN4tR1".into(), DESKTOP_CLIENT_ID.into(), tokens)
    }

    #[tokio::test]
    async fn profile_images_keep_bearer_tokens_on_the_instance_and_reject_redirects_and_oversized_bodies() {
        let (server, _, info) = instance().await;
        let credential = credential(&info);
        let client = ConnectClient::new();
        let own = info.origin.url("/api/v1/account/avatar?v=1");
        server.set_handler(|request| {
            assert_eq!(request.header("Authorization"), Some("Bearer dca_old"));
            (200, "image-bytes".into())
        });
        assert_eq!(client.avatar(&info, &credential, &own).await.unwrap(), b"image-bytes");
        let external = Url::parse("https://avatars.githubusercontent.com/u/1").unwrap();
        let request = client.avatar_request(&info, &credential, &external).build().unwrap();
        assert!(request.headers().get("Authorization").is_none());
        server.set_handler(|_| (302, "https://example.com/avatar".into()));
        assert_eq!(client.avatar(&info, &credential, &own).await.unwrap_err(), ConnectError::Redirected);
        server.set_handler(|_| (200, "x".repeat(MAX_AVATAR_BYTES + 1)));
        assert_eq!(client.avatar(&info, &credential, &own).await.unwrap_err(), ConnectError::InvalidResponse);
        let other = TestServer::start(|_| (200, String::new())).await;
        assert_eq!(client.avatar(&info, &credential, &other.base).await.unwrap_err(), ConnectError::OriginMismatch);
        assert!(other.requests().is_empty());
    }

    #[tokio::test]
    async fn discovery_is_bound_to_the_origin_and_never_redirected() {
        let (server, origin, _) = instance().await;
        let body = document(origin.as_str()).to_string();
        server.set_handler(move |request| {
            assert_eq!((request.method.as_str(), request.path.as_str()), ("GET", "/api/v1/server-info"));
            assert_eq!(request.header("Accept"), Some("application/json"));
            assert!(request.header("Authorization").is_none());
            (200, body.clone())
        });
        let client = ConnectClient::new();
        let info = client.server_info(&origin).await.unwrap();
        assert_eq!(info.origin, origin);

        // A document copied from another instance.
        server.set_handler(|_| (200, document("https://connect.discophile.app").to_string()));
        assert_eq!(client.server_info(&origin).await.unwrap_err(), ConnectError::OriginMismatch);

        let target = TestServer::start(|_| (200, String::new())).await;
        let location = target.server("/api/v1/server-info");
        server.set_handler(move |_| (302, location.clone()));
        assert_eq!(client.server_info(&origin).await.unwrap_err(), ConnectError::Redirected);
        assert!(target.requests().is_empty());

        server.set_handler(|_| (404, String::new()));
        assert_eq!(client.server_info(&origin).await.unwrap_err(), ConnectError::Unsupported);
        server.set_handler(|_| (503, String::new()));
        assert_eq!(client.server_info(&origin).await.unwrap_err(), ConnectError::ServerUnavailable(503));
        server.set_handler(|_| (200, format!("{{\"padding\":\"{}\"}}", "x".repeat(MAX_RESPONSE_BYTES))));
        assert_eq!(client.server_info(&origin).await.unwrap_err(), ConnectError::InvalidResponse);

        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let unreachable = InstanceOrigin::parse(&format!("http://127.0.0.1:{}", closed.local_addr().unwrap().port())).unwrap();
        drop(closed);
        assert_eq!(client.server_info(&unreachable).await.unwrap_err(), ConnectError::Network);
    }

    #[tokio::test]
    async fn code_exchange_posts_the_pkce_form() {
        let (server, _, info) = instance().await;
        let request = AuthorizationRequest::with_values(&info, 50_000, "verifier-fixture".into(), "state-fixture".into()).unwrap();
        let redirect = request.redirect.to_string();
        let resource = info.resource.clone();
        server.set_handler(move |r| {
            assert_eq!((r.method.as_str(), r.path.as_str()), ("POST", "/oauth/token"));
            assert_eq!(r.header("Content-Type"), Some("application/x-www-form-urlencoded"));
            assert!(r.header("Authorization").is_none());
            assert!(String::from_utf8(r.body.clone()).unwrap().contains("code=a%2Bb%26c"));
            assert_eq!(r.form_values("grant_type"), ["authorization_code"]);
            assert_eq!(r.form_values("code"), ["a+b&c"]);
            assert_eq!(r.form_values("redirect_uri"), [redirect.as_str()]);
            assert_eq!(r.form_values("client_id"), ["discophile-desktop"]);
            assert_eq!(r.form_values("code_verifier"), ["verifier-fixture"]);
            assert_eq!(r.form_values("resource"), [resource.as_str()]);
            assert!(r.form_values("client_secret").is_empty());
            (200, TOKENS.into())
        });
        let client = ConnectClient::new();
        let before = crate::now_millis();
        let tokens = client.exchange_code(&info, &request, "a+b&c").await.unwrap();
        assert_eq!(tokens.access_token, "dca_new");
        assert_eq!(tokens.refresh_token, "dcr_new");
        assert_eq!(tokens.scopes, ["account:read", "offline_access"]);
        assert!(tokens.expires_at >= before + 3_600_000 && tokens.expires_at <= crate::now_millis() + 3_600_000);

        server.set_handler(|_| (400, r#"{"error":"invalid_grant","error_description":"expired"}"#.into()));
        assert_eq!(client.exchange_code(&info, &request, "used").await.unwrap_err(), ConnectError::SignInRequired);
        server.set_handler(|r| match r.path.as_str() {
            "/oauth/token" => (200, r#"{"access_token":"dca_only","token_type":"Bearer","expires_in":3600}"#.into()),
            _ => (200, String::new()),
        });
        assert_eq!(client.exchange_code(&info, &request, "c").await.unwrap_err(), ConnectError::InvalidResponse);
        assert_eq!(server.requests().last().unwrap().form_values("token"), ["dca_only"]);
        server.set_handler(|_| (200, r#"{"access_token":"a","token_type":"mac","expires_in":3600,"refresh_token":"r"}"#.into()));
        assert_eq!(client.exchange_code(&info, &request, "c").await.unwrap_err(), ConnectError::InvalidResponse);
        server.set_handler(|_| (200, r#"{"access_token":"a","token_type":"Bearer","expires_in":0,"refresh_token":"r"}"#.into()));
        assert_eq!(client.exchange_code(&info, &request, "c").await.unwrap_err(), ConnectError::InvalidResponse);

        let mut elsewhere = request.clone();
        elsewhere.origin = InstanceOrigin::official();
        let count = server.requests().len();
        assert_eq!(client.exchange_code(&info, &elsewhere, "c").await.unwrap_err(), ConnectError::OriginMismatch);
        assert_eq!(server.requests().len(), count);
    }

    #[tokio::test]
    async fn refresh_rotates_tokens_and_reports_ended_delegations() {
        let (server, _, info) = instance().await;
        let resource = info.resource.clone();
        server.set_handler(move |r| {
            assert_eq!(r.path, "/oauth/token");
            assert!(String::from_utf8(r.body.clone()).unwrap().contains("refresh_token=dcr_old%2B%2F%3D"));
            assert_eq!(r.form_values("grant_type"), ["refresh_token"]);
            assert_eq!(r.form_values("refresh_token"), ["dcr_old+/="]);
            assert_eq!(r.form_values("client_id"), ["discophile-desktop"]);
            assert_eq!(r.form_values("resource"), [resource.as_str()]);
            (200, TOKENS.into())
        });
        let client = ConnectClient::new();
        let old = credential(&info);
        assert!(old.needs_refresh(crate::now_millis()));
        let fresh = client.refresh(&info, &old).await.unwrap();
        assert_eq!((fresh.access_token.as_str(), fresh.refresh_token.as_str()), ("dca_new", "dcr_new"));
        assert_eq!(fresh.namespace(), old.namespace());
        assert_eq!(fresh.instance_id, old.instance_id);
        assert!(!fresh.needs_refresh(crate::now_millis()));

        // Every refresh must rotate the refresh token; the old one is never presented again and the new
        // access token is revoked.
        server.set_handler(|r| match r.path.as_str() {
            "/oauth/token" => (200, r#"{"access_token":"dca_unrotated","token_type":"Bearer","expires_in":3600}"#.into()),
            _ => {
                assert_eq!(r.path, "/oauth/revoke");
                assert_eq!(r.form_values("token"), ["dca_unrotated"]);
                assert_eq!(r.form_values("token_type_hint"), ["access_token"]);
                (200, String::new())
            }
        });
        let count = server.requests().len();
        let unrotated = client.refresh(&info, &old).await.unwrap_err();
        assert_eq!(unrotated, ConnectError::RefreshNotRotated);
        assert!(unrotated.ends_delegation());
        let requests = server.requests();
        assert_eq!(requests[count..].iter().map(|r| r.path.as_str()).collect::<Vec<_>>(), ["/oauth/token", "/oauth/revoke"]);

        server.set_handler(|_| (400, r#"{"error":"invalid_grant"}"#.into()));
        let ended = client.refresh(&info, &old).await.unwrap_err();
        assert_eq!(ended, ConnectError::SignInRequired);
        assert!(ended.ends_delegation());
        for (status, body, expected) in [
            (503, "", ConnectError::ServerUnavailable(503)),
            (400, r#"{"error":"temporarily_unavailable"}"#, ConnectError::ServerUnavailable(400)),
            (429, "", ConnectError::RateLimited { retry_after: None }),
            (401, r#"{"error":"invalid_client"}"#, ConnectError::Rejected("invalid_client".into())),
            (400, r#"{"error":"something_new"}"#, ConnectError::InvalidResponse),
        ] {
            server.set_handler(move |_| (status, body.to_string()));
            let error = client.refresh(&info, &old).await.unwrap_err();
            assert_eq!(error, expected);
            assert!(!error.ends_delegation(), "{error:?} must keep the tokens");
        }
    }

    #[tokio::test]
    async fn account_uses_the_bearer_token_and_checks_the_instance() {
        let (server, _, info) = instance().await;
        server.set_handler(|r| {
            assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", "/api/v1/account"));
            match r.header("Authorization") {
                Some("Bearer dca_old") => (200, example().to_string()),
                _ => (401, r#"{"error":"invalid_token","message":"expired"}"#.into()),
            }
        });
        let client = ConnectClient::new();
        let old = credential(&info);
        let account = client.account_for(&info, &old).await.unwrap();
        assert_eq!(account.account.name, "Ada Example");
        assert_eq!(client.account(&info, "dca_revoked").await.unwrap_err(), ConnectError::Unauthorized);

        let other_account = ConnectCredential { account_id: "u_other".into(), ..old.clone() };
        assert_eq!(client.account_for(&info, &other_account).await.unwrap_err(), ConnectError::InstanceMismatch);

        server.set_handler(|_| {
            let mut value = example();
            value["instanceId"] = json!("replaced-instance");
            (200, value.to_string())
        });
        assert_eq!(client.account(&info, "dca_old").await.unwrap_err(), ConnectError::InstanceMismatch);
        for (status, expected) in [
            (403, ConnectError::InsufficientScope),
            (404, ConnectError::NotOffered),
            (500, ConnectError::ServerUnavailable(500)),
            (429, ConnectError::RateLimited { retry_after: None }),
        ] {
            server.set_handler(move |_| (status, r#"{"error":"x","message":"x"}"#.into()));
            assert_eq!(client.account(&info, "dca_old").await.unwrap_err(), expected);
        }

        // The account endpoint is called only when the capability is advertised.
        let mut no_account = info.clone();
        no_account.capabilities.clear();
        let count = server.requests().len();
        assert_eq!(client.account(&no_account, "dca_old").await.unwrap_err(), ConnectError::NotOffered);
        assert_eq!(server.requests().len(), count);
    }

    #[tokio::test]
    async fn revocation_posts_the_refresh_token() {
        let (server, _, info) = instance().await;
        server.set_handler(|r| {
            assert_eq!((r.method.as_str(), r.path.as_str()), ("POST", "/oauth/revoke"));
            assert_eq!(r.form_values("token"), ["dcr_old+/="]);
            assert_eq!(r.form_values("token_type_hint"), ["refresh_token"]);
            assert_eq!(r.form_values("client_id"), ["discophile-desktop"]);
            (200, String::new())
        });
        let client = ConnectClient::new();
        client.revoke(&info, &credential(&info)).await.unwrap();
        // After the instance identifier changed at the same origin, revocation is still attempted there.
        let replaced = ConnectCredential { instance_id: "previous-instance".into(), ..credential(&info) };
        client.revoke(&info, &replaced).await.unwrap();
        server.set_handler(|_| (503, String::new()));
        assert_eq!(client.revoke(&info, &credential(&info)).await.unwrap_err(), ConnectError::ServerUnavailable(503));
    }

    #[tokio::test]
    async fn tokens_never_reach_another_instance() {
        let (server, _, info) = instance().await;
        server.set_handler(|_| (200, TOKENS.into()));
        let client = ConnectClient::new();
        let mut foreign = credential(&info);
        foreign.origin = InstanceOrigin::parse("http://localhost:1").unwrap();
        assert_eq!(client.refresh(&info, &foreign).await.unwrap_err(), ConnectError::OriginMismatch);
        assert_eq!(client.revoke(&info, &foreign).await.unwrap_err(), ConnectError::OriginMismatch);
        assert_eq!(client.account_for(&info, &foreign).await.unwrap_err(), ConnectError::OriginMismatch);
        let replaced = ConnectCredential { instance_id: "another".into(), ..credential(&info) };
        assert_eq!(client.refresh(&info, &replaced).await.unwrap_err(), ConnectError::InstanceMismatch);
        assert_eq!(client.account_for(&info, &replaced).await.unwrap_err(), ConnectError::InstanceMismatch);
        assert!(server.requests().is_empty());

        // A tampered document is refused before anything is sent.
        let elsewhere = TestServer::start(|_| (200, TOKENS.into())).await;
        let mut tampered = info.clone();
        tampered.token_endpoint = Url::parse(&elsewhere.server("/oauth/token")).unwrap();
        tampered.revocation_endpoint = Url::parse(&elsewhere.server("/oauth/revoke")).unwrap();
        assert_eq!(client.refresh(&tampered, &credential(&info)).await.unwrap_err(), ConnectError::OriginMismatch);
        assert_eq!(client.revoke(&tampered, &credential(&info)).await.unwrap_err(), ConnectError::OriginMismatch);
        assert!(server.requests().is_empty());
        assert!(elsewhere.requests().is_empty());

        // A redirect from the token endpoint is not followed with the refresh token.
        let location = elsewhere.server("/oauth/token");
        server.set_handler(move |_| (307, location.clone()));
        assert_eq!(client.refresh(&info, &credential(&info)).await.unwrap_err(), ConnectError::Redirected);
        assert!(elsewhere.requests().is_empty());
    }
}
