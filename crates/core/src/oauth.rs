//! OAuth helpers shared by the browser sign-ins (ChatGPT and Discophile Connect): PKCE values, form
//! encoding, callback query parsing and the loopback redirect listener's request handling.

use base64::Engine;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use base64::engine::{DecodePaddingMode, general_purpose};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use sha2::{Digest, Sha256};
use url::Url;

/// Largest accepted loopback request.
pub const LOOPBACK_MAX_REQUEST_BYTES: usize = 16_384;

/// Unreserved characters stay; everything else, including `+` and spaces, is percent-encoded.
const COMPONENT: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'.').remove(b'_').remove(b'~');

pub(crate) fn encode(text: &str) -> String {
    utf8_percent_encode(text, COMPONENT).to_string()
}

/// `name=value` pairs sorted by name and percent-encoded, as Discophile sends forms and queries.
pub(crate) fn form(fields: &[(&str, &str)]) -> String {
    let mut fields = fields.to_vec();
    fields.sort_by(|a, b| a.0.cmp(b.0));
    fields.iter().map(|(k, v)| format!("{}={}", encode(k), encode(v))).collect::<Vec<_>>().join("&")
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

/// `base64url(SHA-256(verifier))` without padding (PKCE S256).
pub fn challenge(verifier: &str) -> String {
    base64_url(&Sha256::digest(verifier.as_bytes()))
}

/// 32 bytes from the system's secure random generator, base64url-encoded without padding (43 characters).
pub fn random_token() -> Option<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).ok()?;
    Some(base64_url(&bytes))
}

/// Decoded `name=value` pairs of a query in order, keeping repeated names. A pair without `=` has no value.
pub fn query_items(query: &str) -> Vec<(String, Option<String>)> {
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

/// Reads a raw HTTP request received on a loopback listener and returns the callback URL when it is a
/// single `GET <path>?…` with the exact `Host: 127.0.0.1:<port>` header of `redirect`. Returns `None` while
/// the headers are incomplete or when the request must be rejected.
pub fn parse_loopback_request(raw: &[u8], redirect: &Url, path: &str) -> Option<Url> {
    if raw.len() > LOOPBACK_MAX_REQUEST_BYTES {
        return None;
    }
    let text = std::str::from_utf8(raw).ok()?;
    let head = &text[..text.find("\r\n\r\n")?];
    let mut lines = head.split("\r\n");
    let first: Vec<&str> = lines.next()?.split(' ').filter(|p| !p.is_empty()).collect();
    let port = redirect.port()?;
    let hosts: Vec<&str> = lines.filter(|l| l.to_lowercase().starts_with("host:")).collect();
    if first.len() != 3
        || first[0] != "GET"
        || !first[1].starts_with(&format!("{path}?"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_encoding_sorts_and_escapes_reserved_characters() {
        assert_eq!(form(&[("scope", "a:b c"), ("code", "x+y&z")]), "code=x%2By%26z&scope=a%3Ab%20c");
        assert_eq!(random_token().unwrap().len(), 43);
        assert_eq!(query_items("a=1&b&c=%20"), vec![
            ("a".into(), Some("1".into())),
            ("b".into(), None),
            ("c".into(), Some(" ".into()))
        ]);
    }
}
