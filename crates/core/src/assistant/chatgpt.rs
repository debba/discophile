//! ChatGPT sign-in (OAuth with PKCE through a loopback redirect), identity verification, the model
//! catalog, and streaming Responses API inference with `store: false`.
//!
//! The desktop app owns the loopback listener and secret storage. The flow is:
//! 1. [`PkceRequest::generate`], bind `127.0.0.1:0`, build [`oauth::loopback_redirect`] and open
//!    [`oauth::authorization_url`] in the browser (time out after [`oauth::SIGN_IN_TIMEOUT_SECS`]).
//! 2. Feed each received request to [`oauth::parse_loopback_request`] and [`oauth::callback`], answer with
//!    [`oauth::loopback_response`], then call [`ChatGptClient::exchange_code`].
//! 3. Store the credential with [`ChatGptVault::store`]; before each request call
//!    [`ChatGptClient::current_credential`] and persist it again when it was refreshed.
//!
//! Library credentials never enter these requests.

use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use base64::engine::{DecodePaddingMode, general_purpose};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use url::Url;

use super::edits::AssistantError;
use super::music_assistant::AssistantSchema;

pub const AUTH_BASE: &str = "https://auth.openai.com";
pub const API_BASE: &str = "https://api.openai.com/v1/";
/// Largest accepted response body or event stream.
pub const MAX_RESPONSE_BYTES: usize = 2_000_000;
/// Credentials expiring within this many milliseconds are refreshed first.
pub const REFRESH_MARGIN_MILLIS: i64 = 60_000;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatGptCredential {
    #[serde(rename = "clientID")]
    pub client_id: String,
    pub subject: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(rename = "accessToken")]
    pub access_token: String,
    #[serde(default, rename = "refreshToken")]
    pub refresh_token: Option<String>,
    #[serde(default, rename = "idToken")]
    pub id_token: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    /// Unix milliseconds.
    pub expires: i64,
}

impl fmt::Debug for ChatGptCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatGptCredential")
            .field("client_id", &self.client_id)
            .field("subject", &self.subject)
            .field("email", &self.email)
            .field("scopes", &self.scopes)
            .field("expires", &self.expires)
            .finish_non_exhaustive()
    }
}

impl ChatGptCredential {
    pub fn permits_inference(&self) -> bool {
        self.scopes.iter().any(|s| s == "chatgpt.tokens.use.direct")
            && self.scopes.iter().any(|s| s == "resource.invoke")
    }
    pub fn needs_refresh(&self, now_millis: i64) -> bool {
        self.expires - now_millis <= REFRESH_MARGIN_MILLIS
    }
}

/// A ChatGPT account that signed in before, offered for quick reconnection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatGptRegistration {
    #[serde(rename = "clientID")]
    pub client_id: String,
    pub subject: String,
    #[serde(default)]
    pub email: Option<String>,
}

/// Everything the app keeps in secret storage for ChatGPT.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatGptVault {
    #[serde(rename = "hostID")]
    pub host_id: String,
    #[serde(default)]
    pub registrations: Vec<ChatGptRegistration>,
    #[serde(default)]
    pub credential: Option<ChatGptCredential>,
}

impl Default for ChatGptVault {
    fn default() -> Self {
        Self::new()
    }
}

impl ChatGptVault {
    /// A vault with a new random host identifier.
    pub fn new() -> Self {
        Self {
            host_id: format!("urn:uuid:{}", uuid::Uuid::new_v4().as_hyphenated()),
            registrations: Vec::new(),
            credential: None,
        }
    }

    /// Saves a credential and its registration. Returns true when the account changed, so cached models must be cleared.
    pub fn store(&mut self, credential: ChatGptCredential) -> bool {
        let changed = self
            .credential
            .as_ref()
            .is_none_or(|c| c.client_id != credential.client_id || c.subject != credential.subject);
        self.registrations
            .retain(|r| r.client_id != credential.client_id);
        self.registrations.push(ChatGptRegistration {
            client_id: credential.client_id.clone(),
            subject: credential.subject.clone(),
            email: credential.email.clone(),
        });
        self.credential = Some(credential);
        changed
    }

    /// Forgets the credential locally and returns it for remote revocation.
    pub fn sign_out(&mut self) -> Option<ChatGptCredential> {
        self.credential.take()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatGptModel {
    pub slug: String,
    pub display_name: String,
    pub visibility: String,
}

impl ChatGptModel {
    pub fn id(&self) -> &str {
        &self.slug
    }
}

/// Keeps `selected` when it is still offered, otherwise picks the first model (or nothing).
pub fn choose_model(models: &[ChatGptModel], selected: &str) -> String {
    if models.iter().any(|m| m.slug == selected) {
        selected.to_string()
    } else {
        models.first().map(|m| m.slug.clone()).unwrap_or_default()
    }
}

/// The display name of a model, used as the reply source; "ChatGPT" when unknown.
pub fn model_display_name(models: &[ChatGptModel], id: &str) -> String {
    models
        .iter()
        .find(|m| m.slug == id)
        .map(|m| m.display_name.clone())
        .unwrap_or_else(|| "ChatGPT".to_string())
}

#[derive(Clone, PartialEq, Deserialize)]
pub struct ChatGptTokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub id_token: Option<String>,
    pub expires_in: f64,
    #[serde(default)]
    pub scope: Option<String>,
    pub token_type: String,
}

impl fmt::Debug for ChatGptTokenResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatGptTokenResponse")
            .field("expires_in", &self.expires_in)
            .field("scope", &self.scope)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ChatGptJwk {
    pub kid: String,
    pub kty: String,
    #[serde(default)]
    pub alg: Option<String>,
    #[serde(rename = "use", default)]
    pub key_use: Option<String>,
    #[serde(default)]
    pub n: Option<String>,
    #[serde(default)]
    pub e: Option<String>,
}

/// Random values for one sign-in attempt.
#[derive(Clone, PartialEq)]
pub struct PkceRequest {
    pub verifier: String,
    pub state: String,
    pub nonce: String,
    /// `base64url(SHA-256(verifier))`.
    pub challenge: String,
}

impl fmt::Debug for PkceRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PkceRequest { .. }")
    }
}

impl PkceRequest {
    pub fn generate() -> Result<Self, AssistantError> {
        fn random() -> Result<String, AssistantError> {
            let mut bytes = [0u8; 32];
            getrandom::fill(&mut bytes).map_err(|_| AssistantError::Unavailable)?;
            Ok(oauth::base64_url(&bytes))
        }
        let verifier = random()?;
        Ok(Self {
            challenge: oauth::challenge(&verifier),
            verifier,
            state: random()?,
            nonce: random()?,
        })
    }
}

pub mod oauth {
    use super::*;
    use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};

    pub const RESOURCE: &str = "https://api.openai.com/v1";
    pub const SCOPES: &str =
        "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
    pub const AUTHORIZE: &str = "https://auth.openai.com/api/accounts/authorize";
    pub const ISSUER: &str = "https://auth.openai.com";
    pub const CALLBACK_PATH: &str = "/auth/callback";
    /// Sign-in attempts are abandoned after five minutes.
    pub const SIGN_IN_TIMEOUT_SECS: u64 = 300;
    /// Largest accepted loopback request.
    pub const LOOPBACK_MAX_REQUEST_BYTES: usize = 16_384;
    /// Simultaneous loopback connections accepted while signing in.
    pub const LOOPBACK_MAX_CONNECTIONS: usize = 8;

    /// Unreserved characters stay; everything else, including `+` and spaces, is percent-encoded.
    const COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'.')
        .remove(b'_')
        .remove(b'~');

    pub(crate) fn encode(text: &str) -> String {
        utf8_percent_encode(text, COMPONENT).to_string()
    }

    /// `name=value` pairs sorted by name and percent-encoded, as Discophile sends forms and queries.
    pub(crate) fn form(fields: &[(&str, &str)]) -> String {
        let mut fields = fields.to_vec();
        fields.sort_by(|a, b| a.0.cmp(b.0));
        fields
            .iter()
            .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
            .collect::<Vec<_>>()
            .join("&")
    }

    pub fn base64_url(bytes: &[u8]) -> String {
        general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    }

    pub fn decode_base64_url(text: &str) -> Option<Vec<u8>> {
        const ENGINE: GeneralPurpose = GeneralPurpose::new(
            &base64::alphabet::URL_SAFE,
            GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
        );
        ENGINE.decode(text).ok()
    }

    pub fn challenge(verifier: &str) -> String {
        base64_url(&Sha256::digest(verifier.as_bytes()))
    }

    /// `http://127.0.0.1:<port>/auth/callback`.
    pub fn loopback_redirect(port: u16) -> Url {
        Url::parse(&format!("http://127.0.0.1:{port}{CALLBACK_PATH}")).expect("valid loopback URL")
    }

    pub fn authorization_url(
        host_id: &str,
        redirect: &Url,
        state: &str,
        nonce: &str,
        challenge: &str,
        saved_client_id: Option<&str>,
    ) -> Url {
        let mut items = vec![
            (
                "client_id",
                saved_client_id.unwrap_or("dynamic_agent_client"),
            ),
            ("ext_agent_host_id", host_id),
            ("response_type", "code"),
            ("redirect_uri", redirect.as_str()),
            ("scope", SCOPES),
            ("resource", RESOURCE),
            ("state", state),
            ("nonce", nonce),
            ("code_challenge_method", "S256"),
            ("code_challenge", challenge),
        ];
        if saved_client_id.is_none() {
            items.push(("agent_name_hint", "Discophile"));
        }
        let mut url = Url::parse(AUTHORIZE).expect("valid authorize URL");
        url.set_query(Some(&form(&items)));
        url
    }

    fn query_items(query: &str) -> Vec<(String, Option<String>)> {
        let decode = |s: &str| percent_decode_str(s).decode_utf8_lossy().into_owned();
        query
            .split('&')
            .filter(|p| !p.is_empty())
            .map(|pair| match pair.split_once('=') {
                Some((name, value)) => (decode(name), Some(decode(value))),
                None => (decode(pair), None),
            })
            .collect()
    }

    /// Validates the OAuth redirect and returns `(code, client_id)`. A provider error means the person cancelled.
    pub fn callback(
        url: &Url,
        redirect: &Url,
        state: &str,
        saved_client_id: Option<&str>,
    ) -> Result<(String, String), AssistantError> {
        let invalid = AssistantError::InvalidIdentity;
        if url.scheme() != redirect.scheme()
            || url.host_str() != Some("127.0.0.1")
            || url.port() != redirect.port()
            || url.path() != CALLBACK_PATH
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(invalid);
        }
        let items = query_items(url.query().ok_or(invalid)?);
        let single = |name: &str| -> Result<Option<String>, AssistantError> {
            let values: Vec<&Option<String>> = items
                .iter()
                .filter(|(n, _)| n == name)
                .map(|(_, v)| v)
                .collect();
            if values.len() > 1 {
                return Err(AssistantError::InvalidIdentity);
            }
            Ok(values.first().and_then(|v| (*v).clone()))
        };
        if single("state")?.as_deref() != Some(state) {
            return Err(invalid);
        }
        if single("error")?.is_some() {
            return Err(AssistantError::Cancelled);
        }
        let client = single("client_id")?.or_else(|| saved_client_id.map(str::to_string));
        let code = single("code")?;
        match (code, client) {
            (Some(code), Some(client))
                if !code.is_empty()
                    && client.starts_with("oaiapp_")
                    && client.chars().count() <= 256
                    && saved_client_id.is_none_or(|saved| saved == client) =>
            {
                Ok((code, client))
            }
            _ => Err(invalid),
        }
    }

    /// Reads a raw HTTP request received on the loopback listener and returns the callback URL when it is
    /// a single `GET /auth/callback?…` with the exact `Host: 127.0.0.1:<port>` header. Returns `None` while
    /// the headers are incomplete or when the request must be rejected.
    pub fn parse_loopback_request(raw: &[u8], redirect: &Url) -> Option<Url> {
        if raw.len() > LOOPBACK_MAX_REQUEST_BYTES {
            return None;
        }
        let text = std::str::from_utf8(raw).ok()?;
        let head = &text[..text.find("\r\n\r\n")?];
        let mut lines = head.split("\r\n");
        let first: Vec<&str> = lines.next()?.split(' ').filter(|p| !p.is_empty()).collect();
        let port = redirect.port()?;
        let hosts: Vec<&str> = lines
            .filter(|l| l.to_lowercase().starts_with("host:"))
            .collect();
        if first.len() != 3
            || first[0] != "GET"
            || !first[1].starts_with(&format!("{CALLBACK_PATH}?"))
            || hosts.len() != 1
            || hosts[0][5..].trim() != format!("127.0.0.1:{port}")
        {
            return None;
        }
        Url::parse(&format!("http://127.0.0.1:{port}{}", first[1])).ok()
    }

    /// Whether a partial loopback request has its complete header block.
    pub fn loopback_request_complete(raw: &[u8]) -> bool {
        raw.windows(4).any(|w| w == b"\r\n\r\n")
    }

    /// The page shown after the redirect. No credentials, codes or request values enter it.
    pub fn loopback_response() -> String {
        let body = "<!doctype html><meta name=viewport content='width=device-width'><title>Discophile</title><p>Return to Discophile to finish signing in.</p>";
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nContent-Security-Policy: default-src 'none'\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
    }
}

/// Verified sign-in identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatGptIdentity {
    pub subject: String,
    pub email: Option<String>,
}

/// An RS256 ID token. Claims are exposed only after the signature was verified with the issuer's keys.
#[derive(Debug, Clone)]
pub struct ChatGptIdentityToken {
    pub key_id: String,
    pub message: Vec<u8>,
    pub signature: Vec<u8>,
    claims: Vec<u8>,
}

impl ChatGptIdentityToken {
    pub fn parse(token: &str) -> Result<Self, AssistantError> {
        let invalid = AssistantError::InvalidIdentity;
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 || token.chars().count() >= 32_000 {
            return Err(invalid);
        }
        let header: Value = oauth::decode_base64_url(parts[0])
            .and_then(|h| serde_json::from_slice(&h).ok())
            .ok_or(invalid)?;
        let kid = header
            .get("kid")
            .and_then(Value::as_str)
            .filter(|k| !k.is_empty())
            .ok_or(invalid)?;
        if header.get("alg").and_then(Value::as_str) != Some("RS256") {
            return Err(invalid);
        }
        let claims = oauth::decode_base64_url(parts[1]).ok_or(invalid)?;
        let signature = oauth::decode_base64_url(parts[2])
            .filter(|s| !s.is_empty())
            .ok_or(invalid)?;
        Ok(Self {
            key_id: kid.to_string(),
            message: format!("{}.{}", parts[0], parts[1]).into_bytes(),
            signature,
            claims,
        })
    }

    /// Verifies the RS256 signature with the matching JSON Web Key.
    pub fn verify_signature(&self, keys: &[ChatGptJwk]) -> bool {
        let Some(key) = keys.iter().find(|k| {
            k.kid == self.key_id
                && k.kty == "RSA"
                && k.alg.as_deref().is_none_or(|a| a == "RS256")
                && k.key_use.as_deref().is_none_or(|u| u == "sig")
        }) else {
            return false;
        };
        let n = key
            .n
            .as_deref()
            .and_then(oauth::decode_base64_url)
            .filter(|n| (256..=1024).contains(&n.len()));
        let e = key
            .e
            .as_deref()
            .and_then(oauth::decode_base64_url)
            .filter(|e| !e.is_empty() && e.len() <= 8);
        match (n, e) {
            (Some(n), Some(e)) => verify_rs256(&n, &e, &self.message, &self.signature),
            _ => false,
        }
    }

    /// Checks issuer, audience, nonce, subject and freshness. `now_secs` is Unix seconds.
    pub fn identity(
        &self,
        client_id: &str,
        nonce: Option<&str>,
        expected_subject: Option<&str>,
        signature_valid: bool,
        now_secs: f64,
    ) -> Result<ChatGptIdentity, AssistantError> {
        let invalid = AssistantError::InvalidIdentity;
        if !signature_valid {
            return Err(invalid);
        }
        let value: Value = serde_json::from_slice(&self.claims).map_err(|_| invalid)?;
        let string = |key: &str| value.get(key).and_then(Value::as_str);
        let number = |key: &str| value.get(key).and_then(Value::as_f64);
        let subject = string("sub").filter(|s| !s.is_empty()).ok_or(invalid)?;
        let fresh = string("iss") == Some(oauth::ISSUER)
            && number("exp").is_some_and(|exp| exp > now_secs - 5.0)
            && number("iat").is_some_and(|iat| iat <= now_secs + 5.0);
        let audiences: Vec<&str> = match value.get("aud") {
            Some(Value::Array(items)) => items.iter().filter_map(Value::as_str).collect(),
            Some(Value::String(aud)) => vec![aud.as_str()],
            _ => Vec::new(),
        };
        let valid = fresh
            && audiences.contains(&client_id)
            && (audiences.len() <= 1 || string("azp") == Some(client_id))
            && nonce.is_none_or(|n| string("nonce") == Some(n))
            && expected_subject.is_none_or(|s| s == subject)
            && number("nbf").unwrap_or(0.0) <= now_secs + 5.0;
        if !valid {
            return Err(invalid);
        }
        Ok(ChatGptIdentity {
            subject: subject.to_string(),
            email: string("email").map(str::to_string),
        })
    }
}

/// RSASSA-PKCS1-v1_5 with SHA-256.
pub fn verify_rs256(modulus: &[u8], exponent: &[u8], message: &[u8], signature: &[u8]) -> bool {
    use rsa::{BigUint, Pkcs1v15Sign, RsaPublicKey};
    let Ok(key) = RsaPublicKey::new(
        BigUint::from_bytes_be(modulus),
        BigUint::from_bytes_be(exponent),
    ) else {
        return false;
    };
    key.verify(
        Pkcs1v15Sign::new::<Sha256>(),
        &Sha256::digest(message),
        signature,
    )
    .is_ok()
}

/// Maps a ChatGPT error code to an assistant error.
pub fn failure(code: Option<&str>) -> AssistantError {
    match code {
        Some("subscription_sharing_usage_limit_exceeded" | "rate_limit_exceeded") => {
            AssistantError::UsageLimit
        }
        Some("subscription_sharing_user_not_eligible") => AssistantError::Restricted,
        Some("invalid_grant" | "invalid_token") => AssistantError::SignInRequired,
        _ => AssistantError::Unavailable,
    }
}

fn http_failure(status: u16, body: &[u8]) -> AssistantError {
    let value: Option<Value> = serde_json::from_slice(body).ok();
    let code = value.as_ref().and_then(|v| {
        let error = v.get("error")?;
        error
            .get("code")
            .and_then(Value::as_str)
            .or_else(|| error.as_str())
            .map(str::to_string)
    });
    if let Some(code) = code {
        return failure(Some(&code));
    }
    match status {
        401 => AssistantError::SignInRequired,
        403 => AssistantError::Restricted,
        429 => AssistantError::UsageLimit,
        _ => AssistantError::Unavailable,
    }
}

/// Incremental server-sent events decoding; partial text is never accepted as a completed result.
#[derive(Debug, Clone, Default)]
pub struct ChatGptEventStream {
    pending: Vec<u8>,
    event_data: Vec<String>,
    text: String,
    completed: bool,
    bytes: usize,
}

impl ChatGptEventStream {
    pub fn new() -> Self {
        Self::default()
    }

    /// The text received so far, for live display only.
    pub fn partial_text(&self) -> &str {
        &self.text
    }

    pub fn append(&mut self, data: &[u8]) -> Result<(), AssistantError> {
        self.bytes += data.len();
        if self.bytes > MAX_RESPONSE_BYTES {
            return Err(AssistantError::InvalidResponse);
        }
        self.pending.extend_from_slice(data);
        while let Some(newline) = self.pending.iter().position(|&b| b == b'\n') {
            let mut line: Vec<u8> = self.pending.drain(..=newline).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if line.is_empty() {
                self.consume_event()?;
            } else {
                let string =
                    String::from_utf8(line).map_err(|_| AssistantError::InvalidResponse)?;
                if let Some(data) = string.strip_prefix("data:") {
                    self.event_data
                        .push(data.trim_matches([' ', '\t']).to_string());
                }
            }
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<String, AssistantError> {
        if !self.pending.is_empty() {
            self.append(b"\n")?;
        }
        self.consume_event()?;
        if !self.completed || self.text.trim().is_empty() {
            return Err(AssistantError::Incomplete);
        }
        Ok(self.text)
    }

    fn consume_event(&mut self) -> Result<(), AssistantError> {
        if self.event_data.is_empty() {
            return Ok(());
        }
        let raw = std::mem::take(&mut self.event_data).join("\n");
        if raw == "[DONE]" {
            return Ok(());
        }
        let event: Value =
            serde_json::from_str(&raw).map_err(|_| AssistantError::InvalidResponse)?;
        let kind = event
            .get("type")
            .and_then(Value::as_str)
            .ok_or(AssistantError::InvalidResponse)?;
        match kind {
            "response.output_text.delta" => {
                self.text += event
                    .get("delta")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            }
            "response.refusal.delta" | "response.refusal.done" => {
                return Err(AssistantError::Refused);
            }
            "response.failed" | "error" => {
                let error = event
                    .get("response")
                    .and_then(|r| r.get("error"))
                    .filter(|e| e.is_object())
                    .or_else(|| event.get("error"));
                let code = error
                    .and_then(|e| e.get("code"))
                    .and_then(Value::as_str)
                    .or_else(|| event.get("code").and_then(Value::as_str));
                return Err(failure(code));
            }
            "response.incomplete" => return Err(AssistantError::Incomplete),
            "response.completed" => {
                let response = event
                    .get("response")
                    .filter(|r| r.get("status").and_then(Value::as_str) == Some("completed"));
                let response = response.ok_or(AssistantError::Incomplete)?;
                let content: Vec<&Value> = response
                    .get("output")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .flat_map(|o| {
                        o.get("content")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                    })
                    .collect();
                if content
                    .iter()
                    .any(|c| c.get("type").and_then(Value::as_str) == Some("refusal"))
                {
                    return Err(AssistantError::Refused);
                }
                let output: String = content
                    .iter()
                    .filter(|c| c.get("type").and_then(Value::as_str) == Some("output_text"))
                    .filter_map(|c| c.get("text").and_then(Value::as_str))
                    .collect();
                if !output.is_empty() {
                    self.text = output;
                }
                self.completed = true;
            }
            _ => {}
        }
        Ok(())
    }
}

/// OAuth and inference requests. Uses a client without redirects.
#[derive(Debug, Clone)]
pub struct ChatGptClient {
    http: reqwest::Client,
    auth_base: Url,
    api_base: Url,
}

impl Default for ChatGptClient {
    fn default() -> Self {
        Self::new()
    }
}

impl ChatGptClient {
    pub fn new() -> Self {
        Self::with_endpoints(
            crate::http::server_client(),
            Url::parse(AUTH_BASE).expect("valid auth URL"),
            Url::parse(API_BASE).expect("valid API URL"),
        )
    }

    /// For tests: `api_base` must end with `/`.
    pub fn with_endpoints(http: reqwest::Client, auth_base: Url, api_base: Url) -> Self {
        Self {
            http,
            auth_base,
            api_base,
        }
    }

    fn auth(&self, path: &str) -> Url {
        self.auth_base.join(path).expect("valid auth path")
    }

    async fn data(&self, request: reqwest::RequestBuilder) -> Result<Vec<u8>, AssistantError> {
        let response = request
            .send()
            .await
            .map_err(|_| AssistantError::Unavailable)?;
        let status = response.status().as_u16();
        let body = response
            .bytes()
            .await
            .map_err(|_| AssistantError::Unavailable)?;
        if body.len() > MAX_RESPONSE_BYTES {
            return Err(AssistantError::InvalidResponse);
        }
        if !(200..300).contains(&status) {
            return Err(http_failure(status, &body));
        }
        Ok(body.to_vec())
    }

    fn form_request(&self, url: Url, fields: &[(&str, &str)]) -> reqwest::RequestBuilder {
        self.http
            .post(url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(oauth::form(fields))
    }

    /// Posts to the token endpoint.
    pub async fn token(
        &self,
        fields: &[(&str, &str)],
    ) -> Result<ChatGptTokenResponse, AssistantError> {
        let data = self
            .data(self.form_request(self.auth("/api/accounts/oauth/token"), fields))
            .await?;
        let value: ChatGptTokenResponse =
            serde_json::from_slice(&data).map_err(|_| AssistantError::InvalidResponse)?;
        if !value.token_type.eq_ignore_ascii_case("Bearer")
            || value.access_token.is_empty()
            || !value.expires_in.is_finite()
            || value.expires_in <= 0.0
        {
            return Err(AssistantError::InvalidIdentity);
        }
        Ok(value)
    }

    pub async fn keys(&self) -> Result<Vec<ChatGptJwk>, AssistantError> {
        #[derive(Deserialize)]
        struct Keys {
            keys: Vec<ChatGptJwk>,
        }
        let data = self
            .data(self.http.get(self.auth("/.well-known/jwks.json")))
            .await?;
        serde_json::from_slice::<Keys>(&data)
            .map(|k| k.keys)
            .map_err(|_| AssistantError::InvalidResponse)
    }

    /// Revokes the refresh token. The revocation endpoint must stay on `https://auth.openai.com`.
    pub async fn revoke(&self, credential: &ChatGptCredential) -> Result<(), AssistantError> {
        let token = credential
            .refresh_token
            .as_deref()
            .ok_or(AssistantError::SignInRequired)?;
        let discovery = self
            .data(
                self.http
                    .get(self.auth("/.well-known/openid-configuration")),
            )
            .await?;
        let metadata: Value =
            serde_json::from_slice(&discovery).map_err(|_| AssistantError::InvalidResponse)?;
        let url = metadata
            .get("revocation_endpoint")
            .and_then(Value::as_str)
            .and_then(|e| Url::parse(e).ok())
            .filter(|u| {
                u.scheme() == "https"
                    && u.host_str() == Some("auth.openai.com")
                    && u.port().is_none()
                    && u.username().is_empty()
                    && u.password().is_none()
                    && u.fragment().is_none()
            })
            .ok_or(AssistantError::InvalidResponse)?;
        let fields = [
            ("token", token),
            ("token_type_hint", "refresh_token"),
            ("client_id", credential.client_id.as_str()),
        ];
        let body = fields
            .iter()
            .map(|(k, v)| format!("{k}={}", oauth::encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        self.data(
            self.http
                .post(url)
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(body),
        )
        .await?;
        Ok(())
    }

    /// Models offered to this account, in server order.
    pub async fn models(&self, token: &str) -> Result<Vec<ChatGptModel>, AssistantError> {
        #[derive(Deserialize)]
        struct Catalog {
            models: Vec<ChatGptModel>,
        }
        let url = self.api_base.join("models").expect("valid models URL");
        let data = self.data(self.http.get(url).bearer_auth(token)).await?;
        let catalog: Catalog =
            serde_json::from_slice(&data).map_err(|_| AssistantError::InvalidResponse)?;
        Ok(catalog
            .models
            .into_iter()
            .filter(|m| m.visibility == "list" && !m.slug.is_empty())
            .collect())
    }

    /// The JSON body of a Responses API request: streaming, strict JSON schema and `store: false`.
    pub fn response_body(
        model: &str,
        instructions: &str,
        input: &str,
        schema: &AssistantSchema,
    ) -> Value {
        json!({
            "model": model,
            "instructions": instructions,
            "input": [{"role": "user", "content": input}],
            "store": false,
            "stream": true,
            "text": {"format": {"type": "json_schema", "name": "discophile_music", "strict": true, "schema": schema.json()}},
        })
    }

    /// Streams one structured response and returns its final text.
    pub async fn respond(
        &self,
        token: &str,
        model: &str,
        instructions: &str,
        input: &str,
        schema: &AssistantSchema,
    ) -> Result<String, AssistantError> {
        let url = self
            .api_base
            .join("responses")
            .expect("valid responses URL");
        let response = self
            .http
            .post(url)
            .bearer_auth(token)
            .header("Accept", "text/event-stream")
            .json(&Self::response_body(model, instructions, input, schema))
            .send()
            .await
            .map_err(|_| AssistantError::Unavailable)?;
        let status = response.status().as_u16();
        let mut body = response.bytes_stream();
        if status != 200 {
            let mut data = Vec::new();
            while let Some(Ok(chunk)) = body.next().await {
                data.extend_from_slice(&chunk);
                if data.len() >= 16_384 {
                    break;
                }
            }
            return Err(http_failure(status, &data));
        }
        let mut stream = ChatGptEventStream::new();
        while let Some(chunk) = body.next().await {
            stream.append(&chunk.map_err(|_| AssistantError::Unavailable)?)?;
        }
        stream.finish()
    }

    /// Validates a token response: verifies the ID token (when present) and requires inference consent.
    pub async fn validate(
        &self,
        tokens: ChatGptTokenResponse,
        client_id: &str,
        nonce: Option<&str>,
        expected_subject: Option<&str>,
        previous: Option<&ChatGptCredential>,
    ) -> Result<ChatGptCredential, AssistantError> {
        let identity = match (&tokens.id_token, previous) {
            (Some(id_token), _) => {
                let token = ChatGptIdentityToken::parse(id_token)?;
                let keys = self.keys().await?;
                let valid = token.verify_signature(&keys);
                token.identity(
                    client_id,
                    nonce,
                    expected_subject,
                    valid,
                    crate::now_millis() as f64 / 1_000.0,
                )?
            }
            (None, Some(previous)) if nonce.is_none() => ChatGptIdentity {
                subject: previous.subject.clone(),
                email: previous.email.clone(),
            },
            _ => return Err(AssistantError::InvalidIdentity),
        };
        let scopes = match &tokens.scope {
            Some(scope) => scope
                .split(' ')
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect(),
            None => previous.map(|p| p.scopes.clone()).unwrap_or_default(),
        };
        let credential = ChatGptCredential {
            client_id: client_id.to_string(),
            subject: identity.subject,
            email: identity.email,
            access_token: tokens.access_token,
            refresh_token: tokens
                .refresh_token
                .or_else(|| previous.and_then(|p| p.refresh_token.clone())),
            id_token: tokens
                .id_token
                .or_else(|| previous.map(|p| p.id_token.clone()))
                .unwrap_or_default(),
            scopes,
            expires: crate::now_millis() + (tokens.expires_in * 1_000.0) as i64,
        };
        if !credential.permits_inference() {
            return Err(AssistantError::ConsentRequired);
        }
        Ok(credential)
    }

    /// Completes sign-in with the code from [`oauth::callback`].
    #[allow(clippy::too_many_arguments)]
    pub async fn exchange_code(
        &self,
        code: &str,
        client_id: &str,
        pkce: &PkceRequest,
        redirect: &Url,
        expected_subject: Option<&str>,
    ) -> Result<ChatGptCredential, AssistantError> {
        let tokens = self
            .token(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("client_id", client_id),
                ("code_verifier", &pkce.verifier),
                ("redirect_uri", redirect.as_str()),
                ("resource", oauth::RESOURCE),
            ])
            .await?;
        self.validate(tokens, client_id, Some(&pkce.nonce), expected_subject, None)
            .await
    }

    pub async fn refresh(
        &self,
        credential: &ChatGptCredential,
    ) -> Result<ChatGptCredential, AssistantError> {
        let token = credential
            .refresh_token
            .as_deref()
            .ok_or(AssistantError::SignInRequired)?;
        let tokens = self
            .token(&[
                ("grant_type", "refresh_token"),
                ("client_id", &credential.client_id),
                ("refresh_token", token),
                ("resource", oauth::RESOURCE),
            ])
            .await?;
        self.validate(
            tokens,
            &credential.client_id,
            None,
            Some(&credential.subject),
            Some(credential),
        )
        .await
    }

    /// A usable credential, refreshed when it expires within a minute. The bool is true when it changed
    /// and must be stored again.
    pub async fn current_credential(
        &self,
        credential: Option<&ChatGptCredential>,
    ) -> Result<(ChatGptCredential, bool), AssistantError> {
        let credential = credential.ok_or(AssistantError::SignInRequired)?;
        if !credential.permits_inference() {
            return Err(AssistantError::ConsentRequired);
        }
        if !credential.needs_refresh(crate::now_millis()) {
            return Ok((credential.clone(), false));
        }
        Ok((self.refresh(credential).await?, true))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::assistant::music_assistant::AssistantSchema;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A recorded HTTP request.
    #[derive(Debug, Clone)]
    pub(crate) struct Recorded {
        pub method: String,
        pub path: String,
        pub headers: Vec<(String, String)>,
        pub body: Vec<u8>,
    }

    impl Recorded {
        pub fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str())
        }
    }

    type Handler = Arc<dyn Fn(&Recorded) -> (u16, Vec<u8>) + Send + Sync>;

    /// A one-request-per-connection HTTP server on 127.0.0.1 for client tests.
    pub(crate) async fn serve(
        handler: impl Fn(&Recorded) -> (u16, Vec<u8>) + Send + Sync + 'static,
    ) -> (Url, Arc<Mutex<Vec<Recorded>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let log = Arc::new(Mutex::new(Vec::new()));
        let handler: Handler = Arc::new(handler);
        let requests = log.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let handler = handler.clone();
                let requests = requests.clone();
                tokio::spawn(async move {
                    let mut data = Vec::new();
                    let mut buffer = [0u8; 4096];
                    let (head_end, length) = loop {
                        let n = socket.read(&mut buffer).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        data.extend_from_slice(&buffer[..n]);
                        if let Some(end) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&data[..end]).to_lowercase();
                            let length = head
                                .lines()
                                .find_map(|l| {
                                    l.strip_prefix("content-length:")
                                        .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                                })
                                .unwrap_or(0);
                            break (end + 4, length);
                        }
                    };
                    while data.len() < head_end + length {
                        let n = socket.read(&mut buffer).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        data.extend_from_slice(&buffer[..n]);
                    }
                    let head = String::from_utf8_lossy(&data[..head_end - 4]).to_string();
                    let mut lines = head.split("\r\n");
                    let first: Vec<String> = lines
                        .next()
                        .unwrap_or_default()
                        .split(' ')
                        .map(str::to_string)
                        .collect();
                    let headers = lines
                        .filter_map(|l| l.split_once(':'))
                        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
                        .collect();
                    let request = Recorded {
                        method: first.first().cloned().unwrap_or_default(),
                        path: first.get(1).cloned().unwrap_or_default(),
                        headers,
                        body: data[head_end..].to_vec(),
                    };
                    let (status, body) = handler(&request);
                    requests.lock().unwrap().push(request);
                    let reply = format!(
                        "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = socket.write_all(reply.as_bytes()).await;
                    let _ = socket.write_all(&body).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        (base, log)
    }

    fn client(base: &Url) -> ChatGptClient {
        ChatGptClient::with_endpoints(
            reqwest::Client::new(),
            base.clone(),
            base.join("v1/").unwrap(),
        )
    }

    pub(crate) fn event(value: Value) -> Vec<u8> {
        format!("data: {value}\n\n").into_bytes()
    }

    pub(crate) fn completed(text: &str) -> Vec<u8> {
        event(
            json!({"type": "response.completed", "response": {"status": "completed", "output": [
            {"type": "message", "content": [{"type": "output_text", "text": text}]}]}}),
        )
    }

    #[test]
    fn stream_handles_byte_splits_crlf_and_authoritative_completion() {
        let mut stream = ChatGptEventStream::new();
        let mut data = event(json!({"type": "response.output_text.delta", "delta": "partial"}));
        data.extend(completed("Città 🎵"));
        let data = String::from_utf8(data).unwrap().replace('\n', "\r\n");
        for byte in data.as_bytes() {
            stream.append(&[*byte]).unwrap();
        }
        assert_eq!(stream.finish().unwrap(), "Città 🎵");
    }

    #[test]
    fn stream_never_accepts_partial_text_or_done_as_success() {
        let mut stream = ChatGptEventStream::new();
        stream
            .append(&event(
                json!({"type": "response.output_text.delta", "delta": "partial"}),
            ))
            .unwrap();
        stream.append(b"data: [DONE]\n\n").unwrap();
        assert_eq!(stream.finish().unwrap_err(), AssistantError::Incomplete);
    }

    #[test]
    fn stream_reports_late_usage_failure_and_refusal() {
        let mut stream = ChatGptEventStream::new();
        stream.append(&completed("partial")).unwrap();
        let failed = event(
            json!({"type": "response.failed", "response": {"error": {"code": "subscription_sharing_usage_limit_exceeded"}}}),
        );
        assert_eq!(
            stream.append(&failed).unwrap_err(),
            AssistantError::UsageLimit
        );
        let mut direct = ChatGptEventStream::new();
        assert_eq!(
            direct
                .append(&event(
                    json!({"type": "error", "code": "subscription_sharing_user_not_eligible"})
                ))
                .unwrap_err(),
            AssistantError::Restricted
        );
        let mut refusal = ChatGptEventStream::new();
        assert_eq!(
            refusal
                .append(&event(
                    json!({"type": "response.refusal.delta", "delta": "No"})
                ))
                .unwrap_err(),
            AssistantError::Refused
        );
    }

    #[test]
    fn oauth_authorization_uses_pkce_and_explicit_inference_consent() {
        let redirect = oauth::loopback_redirect(12345);
        let url = oauth::authorization_url(
            "urn:uuid:fixture",
            &redirect,
            "state",
            "nonce",
            "challenge",
            None,
        );
        let value = |name: &str| {
            url.query_pairs()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.into_owned())
        };
        assert_eq!(url.host_str(), Some("auth.openai.com"));
        assert_eq!(url.path(), "/api/accounts/authorize");
        assert_eq!(value("client_id").as_deref(), Some("dynamic_agent_client"));
        assert_eq!(value("redirect_uri").as_deref(), Some(redirect.as_str()));
        assert_eq!(value("code_challenge_method").as_deref(), Some("S256"));
        assert_eq!(value("code_challenge").as_deref(), Some("challenge"));
        assert_eq!(value("state").as_deref(), Some("state"));
        assert_eq!(value("nonce").as_deref(), Some("nonce"));
        assert_eq!(value("resource").as_deref(), Some(oauth::RESOURCE));
        assert!(
            value("scope")
                .unwrap()
                .contains("chatgpt.tokens.use.direct")
        );
        assert!(value("client_secret").is_none());
        assert!(url.query().unwrap().contains("scope=openid%20profile"));
        let saved = oauth::authorization_url("h", &redirect, "s", "n", "c", Some("oaiapp_saved"));
        assert!(!saved.query().unwrap().contains("agent_name_hint"));
    }

    #[test]
    fn pkce_challenge_is_sha256_of_the_verifier() {
        // RFC 7636 appendix B.
        assert_eq!(
            oauth::challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let request = PkceRequest::generate().unwrap();
        assert_eq!(request.challenge, oauth::challenge(&request.verifier));
        assert_ne!(request.state, request.nonce);
        assert_eq!(
            oauth::decode_base64_url(&request.verifier).unwrap().len(),
            32
        );
    }

    #[test]
    fn callback_rejects_wrong_state_duplicate_parameters_and_registration_switch() {
        let redirect = oauth::loopback_redirect(12345);
        let callback = |suffix: &str, saved: Option<&str>| {
            oauth::callback(
                &Url::parse(&format!("{redirect}{suffix}")).unwrap(),
                &redirect,
                "expected",
                saved,
            )
            .map(|r| r.1)
        };
        assert_eq!(
            callback(
                "?state=expected&code=fixture&client_id=oaiapp_fixture",
                None
            )
            .unwrap(),
            "oaiapp_fixture"
        );
        assert_eq!(
            callback("?state=expected&code=fixture", Some("oaiapp_saved")).unwrap(),
            "oaiapp_saved"
        );
        for query in [
            "?state=wrong&code=fixture&client_id=oaiapp_fixture",
            "?state=expected&state=expected&code=fixture&client_id=oaiapp_fixture",
            "?state=expected&code=a&code=b&client_id=oaiapp_fixture",
            "?state=expected&code=a&client_id=dynamic_agent_client",
        ] {
            assert!(callback(query, None).is_err(), "{query}");
        }
        assert!(
            callback(
                "?state=expected&code=fixture&client_id=oaiapp_other",
                Some("oaiapp_saved")
            )
            .is_err()
        );
        assert_eq!(
            callback("?state=expected&error=access_denied", None).unwrap_err(),
            AssistantError::Cancelled
        );
        let localhost = Url::parse(
            "http://localhost:12345/auth/callback?state=expected&code=a&client_id=oaiapp_fixture",
        )
        .unwrap();
        assert!(oauth::callback(&localhost, &redirect, "expected", None).is_err());
    }

    #[test]
    fn loopback_accepts_only_the_exact_callback_request() {
        let redirect = oauth::loopback_redirect(4567);
        let request = |line: &str, host: &str| {
            format!("{line}\r\nHost: {host}\r\nUser-Agent: x\r\n\r\n").into_bytes()
        };
        let url = oauth::parse_loopback_request(
            &request(
                "GET /auth/callback?state=s&code=c HTTP/1.1",
                "127.0.0.1:4567",
            ),
            &redirect,
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "http://127.0.0.1:4567/auth/callback?state=s&code=c"
        );
        assert!(
            oauth::parse_loopback_request(
                &request("POST /auth/callback?state=s HTTP/1.1", "127.0.0.1:4567"),
                &redirect
            )
            .is_none()
        );
        assert!(
            oauth::parse_loopback_request(
                &request("GET /other?state=s HTTP/1.1", "127.0.0.1:4567"),
                &redirect
            )
            .is_none()
        );
        assert!(
            oauth::parse_loopback_request(
                &request("GET /auth/callback?state=s HTTP/1.1", "localhost:4567"),
                &redirect
            )
            .is_none()
        );
        assert!(
            oauth::parse_loopback_request(
                b"GET /auth/callback?state=s HTTP/1.1\r\nHost: 127.0.0.1:4567\r\n",
                &redirect
            )
            .is_none()
        );
        assert!(!oauth::loopback_request_complete(b"GET / HTTP/1.1\r\n"));
        assert!(oauth::loopback_response().contains("Content-Security-Policy: default-src 'none'"));
    }

    fn identity_token(changes: Value, signature: &str) -> String {
        let mut claims = json!({"iss": "https://auth.openai.com", "sub": "fixture-sub", "aud": "oaiapp_fixture", "nonce": "nonce", "iat": 1_000.0, "exp": 2_000.0});
        for (k, v) in changes.as_object().unwrap() {
            claims[k] = v.clone();
        }
        let header = oauth::base64_url(
            json!({"alg": "RS256", "kid": "fixture"})
                .to_string()
                .as_bytes(),
        );
        let payload = oauth::base64_url(claims.to_string().as_bytes());
        format!("{header}.{payload}.{signature}")
    }

    #[test]
    fn identity_requires_signature_issuer_audience_nonce_subject_and_freshness() {
        let identity = |changes: Value, signature: bool| {
            ChatGptIdentityToken::parse(&identity_token(changes, "c2ln"))
                .unwrap()
                .identity(
                    "oaiapp_fixture",
                    Some("nonce"),
                    Some("fixture-sub"),
                    signature,
                    1_500.0,
                )
                .map(|i| i.subject)
        };
        assert_eq!(identity(json!({}), true).unwrap(), "fixture-sub");
        assert!(identity(json!({}), false).is_err());
        for claims in [
            json!({"iss": "https://attacker.test"}),
            json!({"aud": "other"}),
            json!({"nonce": "other"}),
            json!({"sub": "other"}),
            json!({"exp": 1_000.0}),
            json!({"iat": 1_600.0}),
            json!({"nbf": 1_600.0}),
            json!({"aud": ["oaiapp_fixture", "other"]}),
        ] {
            assert!(identity(claims.clone(), true).is_err(), "{claims}");
        }
        assert_eq!(
            identity(
                json!({"aud": ["oaiapp_fixture", "other"], "azp": "oaiapp_fixture"}),
                true
            )
            .unwrap(),
            "fixture-sub"
        );
        assert!(ChatGptIdentityToken::parse("eyJhbGciOiJub25lIn0.e30.c2ln").is_err());
    }

    // A synthetic 2048-bit key generated for this test only, and its signature over `MESSAGE`.
    const MODULUS: &str = include_str!("testdata/rs256_modulus.txt");
    const SIGNATURE: &str = include_str!("testdata/rs256_signature.txt");
    const MESSAGE: &str = "header.payload";

    #[test]
    fn rs256_signatures_verify_only_with_the_matching_key_and_message() {
        let n = oauth::decode_base64_url(MODULUS.trim()).unwrap();
        let signature = oauth::decode_base64_url(SIGNATURE.trim()).unwrap();
        let e = [1u8, 0, 1];
        assert!(verify_rs256(&n, &e, MESSAGE.as_bytes(), &signature));
        assert!(!verify_rs256(&n, &e, b"header.other", &signature));
        let mut tampered = signature.clone();
        tampered[10] ^= 1;
        assert!(!verify_rs256(&n, &e, MESSAGE.as_bytes(), &tampered));
        let token = ChatGptIdentityToken {
            key_id: "k".into(),
            message: MESSAGE.as_bytes().to_vec(),
            signature,
            claims: Vec::new(),
        };
        let key = |kid: &str, kty: &str, alg: Option<&str>| ChatGptJwk {
            kid: kid.into(),
            kty: kty.into(),
            alg: alg.map(str::to_string),
            key_use: Some("sig".into()),
            n: Some(MODULUS.trim().into()),
            e: Some("AQAB".into()),
        };
        assert!(
            token.verify_signature(&[key("other", "RSA", None), key("k", "RSA", Some("RS256"))])
        );
        assert!(!token.verify_signature(&[key("k", "EC", None)]));
        assert!(!token.verify_signature(&[key("k", "RSA", Some("RS512"))]));
    }

    #[tokio::test]
    async fn models_respect_server_order_and_visibility() {
        let (base, log) = serve(|_| {
            (200, br#"{"models":[{"slug":"second","display_name":"Second","visibility":"list"},{"slug":"hidden","display_name":"Hidden","visibility":"hide"},{"slug":"first","display_name":"First","visibility":"list"}]}"#.to_vec())
        })
        .await;
        let models = client(&base).models("fixture-token").await.unwrap();
        assert_eq!(
            models.iter().map(|m| m.id()).collect::<Vec<_>>(),
            ["second", "first"]
        );
        let request = log.lock().unwrap()[0].clone();
        assert_eq!(request.path, "/v1/models");
        assert_eq!(
            request.header("Authorization"),
            Some("Bearer fixture-token")
        );
        assert_eq!(choose_model(&models, "first"), "first");
        assert_eq!(choose_model(&models, "gone"), "second");
        assert_eq!(model_display_name(&models, "first"), "First");
    }

    #[tokio::test]
    async fn inference_uses_public_endpoint_and_supported_streaming_fields() {
        let (base, log) = serve(|_| (200, completed("fixture-result"))).await;
        let result = client(&base)
            .respond(
                "fixture-token",
                "available-model",
                "Fixture instructions",
                "Fixture request",
                &AssistantSchema::Plan,
            )
            .await;
        assert_eq!(result.unwrap(), "fixture-result");
        let request = log.lock().unwrap()[0].clone();
        assert_eq!(
            (request.method.as_str(), request.path.as_str()),
            ("POST", "/v1/responses")
        );
        assert_eq!(
            request.header("Authorization"),
            Some("Bearer fixture-token")
        );
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        let mut keys: Vec<&String> = body.as_object().unwrap().keys().collect();
        keys.sort();
        assert_eq!(
            keys,
            ["input", "instructions", "model", "store", "stream", "text"]
        );
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
        assert_eq!(body["text"]["format"]["type"], "json_schema");
        assert_eq!(body["text"]["format"]["strict"], true);
    }

    #[tokio::test]
    async fn oauth_token_encoding_preserves_plus_and_reports_revoked_refresh_grant() {
        let (base, log) = serve(|_| {
            (
                400,
                br#"{"error":"invalid_grant","error_description":"Never expose this"}"#.to_vec(),
            )
        })
        .await;
        assert_eq!(
            client(&base)
                .token(&[("refresh_token", "a+b&c")])
                .await
                .unwrap_err(),
            AssistantError::SignInRequired
        );
        let request = log.lock().unwrap()[0].clone();
        assert_eq!(request.path, "/api/accounts/oauth/token");
        assert!(request.header("Authorization").is_none());
        assert!(
            String::from_utf8(request.body)
                .unwrap()
                .contains("refresh_token=a%2Bb%26c")
        );
    }

    #[tokio::test]
    async fn revocation_rejects_discovery_redirect_to_untrusted_host() {
        let (base, log) = serve(|_| {
            (
                200,
                br#"{"revocation_endpoint":"https://attacker.test/revoke"}"#.to_vec(),
            )
        })
        .await;
        let credential = ChatGptCredential {
            client_id: "oaiapp_fixture".into(),
            subject: "fixture".into(),
            email: None,
            access_token: "fixture".into(),
            refresh_token: Some("fixture-refresh".into()),
            id_token: "fixture".into(),
            scopes: vec![],
            expires: 0,
        };
        assert_eq!(
            client(&base).revoke(&credential).await.unwrap_err(),
            AssistantError::InvalidResponse
        );
        let requests = log.lock().unwrap().clone();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "/.well-known/openid-configuration");
    }

    #[tokio::test]
    async fn refresh_keeps_identity_and_requires_inference_scopes() {
        let (base, _) = serve(|request| {
            let body = String::from_utf8(request.body.clone()).unwrap();
            assert!(body.contains("grant_type=refresh_token"));
            (200, br#"{"access_token":"new","expires_in":3600,"token_type":"bearer","scope":"openid resource.invoke chatgpt.tokens.use.direct"}"#.to_vec())
        })
        .await;
        let old = ChatGptCredential {
            client_id: "oaiapp_fixture".into(),
            subject: "fixture".into(),
            email: Some("a@example.test".into()),
            access_token: "old".into(),
            refresh_token: Some("refresh".into()),
            id_token: "id".into(),
            scopes: vec!["resource.invoke".into(), "chatgpt.tokens.use.direct".into()],
            expires: 0,
        };
        let (fresh, refreshed) = client(&base).current_credential(Some(&old)).await.unwrap();
        assert!(refreshed);
        assert_eq!(fresh.access_token, "new");
        assert_eq!(fresh.subject, "fixture");
        assert_eq!(fresh.refresh_token.as_deref(), Some("refresh"));
        assert!(!fresh.needs_refresh(crate::now_millis()));
        assert!(!format!("{fresh:?}").contains("new"));
        let mut vault = ChatGptVault::new();
        assert!(vault.host_id.starts_with("urn:uuid:"));
        assert!(vault.store(fresh.clone()));
        assert!(!vault.store(fresh));
        assert_eq!(vault.registrations.len(), 1);
        let unconsented = ChatGptCredential {
            scopes: vec![],
            ..old
        };
        assert_eq!(
            client(&base)
                .current_credential(Some(&unconsented))
                .await
                .unwrap_err(),
            AssistantError::ConsentRequired
        );
        assert_eq!(
            client(&base).current_credential(None).await.unwrap_err(),
            AssistantError::SignInRequired
        );
    }
}
