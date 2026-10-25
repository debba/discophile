//! Connect instance addresses. An instance is identified by its canonical origin: the scheme, the lowercase
//! host and the port, with the default port omitted. Every token and identifier belongs to one origin.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::{Host, Url};

/// The official instance, used until the person chooses another one.
pub const DEFAULT_INSTANCE: &str = "https://connect.discophile.app";

/// Why an instance address was refused. The app maps each variant to a localized message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum InstanceUrlError {
    #[error("empty address")]
    Empty,
    #[error("not a valid address")]
    Invalid,
    /// `http` outside the loopback development exception, or another scheme.
    #[error("insecure address")]
    Insecure,
    #[error("address contains a user name or password")]
    Credentials,
    /// A path other than `/`, a query or a fragment: an instance is only an origin.
    #[error("address has a path, query or fragment")]
    NotOrigin,
}

/// A validated, canonical instance origin such as `https://connect.discophile.app`.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct InstanceOrigin {
    origin: String,
    insecure: bool,
}

impl InstanceOrigin {
    /// The official instance.
    pub fn official() -> Self {
        Self::parse(DEFAULT_INSTANCE).expect("valid default instance")
    }

    /// Validates an address typed by the person. A missing scheme means `https`. Only `https` is accepted,
    /// except `http` for `localhost`, `127.0.0.1` and `[::1]`, which is flagged as unencrypted.
    pub fn parse(input: &str) -> Result<Self, InstanceUrlError> {
        let input = input.trim();
        if input.is_empty() {
            return Err(InstanceUrlError::Empty);
        }
        let text = if input.contains("://") { input.to_string() } else { format!("https://{input}") };
        let url = Url::parse(&text).map_err(|_| InstanceUrlError::Invalid)?;
        let host = url.host().ok_or(InstanceUrlError::Invalid)?;
        let insecure = match url.scheme() {
            "https" => false,
            "http" if is_loopback(&host) => true,
            "http" => return Err(InstanceUrlError::Insecure),
            _ => return Err(InstanceUrlError::Invalid),
        };
        if !url.username().is_empty() || url.password().is_some() {
            return Err(InstanceUrlError::Credentials);
        }
        if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
            return Err(InstanceUrlError::NotOrigin);
        }
        // Lowercase host (punycode for international names) and no default port.
        Ok(Self { origin: url.origin().ascii_serialization(), insecure })
    }

    /// The canonical origin, without a trailing slash.
    pub fn as_str(&self) -> &str {
        &self.origin
    }

    /// True for the loopback `http` development exception: the connection is not encrypted.
    pub fn is_insecure(&self) -> bool {
        self.insecure
    }

    pub fn is_official(&self) -> bool {
        self.origin == DEFAULT_INSTANCE
    }

    /// An absolute URL on this instance, such as `/api/v1/account`.
    pub fn url(&self, path: &str) -> Url {
        Url::parse(&format!("{}{path}", self.origin)).expect("valid instance URL")
    }

    /// Whether `url` has this origin: same scheme, host and port.
    pub fn same_origin(&self, url: &Url) -> bool {
        url.origin().ascii_serialization() == self.origin
    }

    /// Whether `url` belongs to this instance: same scheme, host and port, and no user name, password or
    /// fragment.
    pub fn owns(&self, url: &Url) -> bool {
        self.same_origin(url)
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none()
    }
}

impl Default for InstanceOrigin {
    fn default() -> Self {
        Self::official()
    }
}

fn is_loopback(host: &Host<&str>) -> bool {
    match host {
        Host::Domain(name) => *name == "localhost",
        Host::Ipv4(address) => address.octets() == [127, 0, 0, 1],
        Host::Ipv6(address) => address.is_loopback(),
    }
}

impl TryFrom<String> for InstanceOrigin {
    type Error = InstanceUrlError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<InstanceOrigin> for String {
    fn from(value: InstanceOrigin) -> Self {
        value.origin
    }
}

impl fmt::Display for InstanceOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.origin)
    }
}

impl fmt::Debug for InstanceOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "InstanceOrigin({})", self.origin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin(input: &str) -> Result<String, InstanceUrlError> {
        InstanceOrigin::parse(input).map(|o| o.as_str().to_string())
    }

    #[test]
    fn canonicalizes_scheme_host_and_port() {
        assert_eq!(origin("HTTPS://Connect.Example.ORG:443/").unwrap(), "https://connect.example.org");
        assert_eq!(origin("  connect.example.org  ").unwrap(), "https://connect.example.org");
        assert_eq!(origin("https://connect.example.org:8443").unwrap(), "https://connect.example.org:8443");
        assert_eq!(origin("https://bücher.example").unwrap(), "https://xn--bcher-kva.example");
        let official = InstanceOrigin::official();
        assert_eq!(official.as_str(), DEFAULT_INSTANCE);
        assert!(official.is_official() && !official.is_insecure());
        assert_eq!(InstanceOrigin::default(), official);
        assert!(!InstanceOrigin::parse("https://connect.discophile.app.example").unwrap().is_official());
        assert_eq!(official.url("/api/v1/server-info").as_str(), "https://connect.discophile.app/api/v1/server-info");
    }

    #[test]
    fn allows_http_only_for_loopback_and_marks_it_insecure() {
        for (input, expected) in [
            ("http://localhost:3000", "http://localhost:3000"),
            ("http://LOCALHOST", "http://localhost"),
            ("http://127.0.0.1:8080/", "http://127.0.0.1:8080"),
            ("http://[::1]:3000", "http://[::1]:3000"),
            ("http://localhost:80", "http://localhost"),
        ] {
            let parsed = InstanceOrigin::parse(input).unwrap();
            assert_eq!(parsed.as_str(), expected);
            assert!(parsed.is_insecure(), "{input}");
        }
        assert!(!InstanceOrigin::parse("https://localhost:3000").unwrap().is_insecure());
        for input in ["http://connect.example.org", "http://192.168.1.20:3000", "http://localhost.example.org", "http://127.0.0.2"] {
            assert_eq!(origin(input).unwrap_err(), InstanceUrlError::Insecure, "{input}");
        }
        assert_eq!(origin("ftp://connect.example.org").unwrap_err(), InstanceUrlError::Invalid);
    }

    #[test]
    fn rejects_credentials_paths_queries_and_fragments() {
        assert_eq!(origin("").unwrap_err(), InstanceUrlError::Empty);
        assert_eq!(origin("   ").unwrap_err(), InstanceUrlError::Empty);
        assert_eq!(origin("https://").unwrap_err(), InstanceUrlError::Invalid);
        assert_eq!(origin("https://user:secret@connect.example.org").unwrap_err(), InstanceUrlError::Credentials);
        assert_eq!(origin("https://user@connect.example.org").unwrap_err(), InstanceUrlError::Credentials);
        for input in [
            "https://connect.example.org/connect",
            "https://connect.example.org//",
            "https://connect.example.org/?",
            "https://connect.example.org/?a=1",
            "https://connect.example.org/#",
            "https://connect.example.org#top",
        ] {
            assert_eq!(origin(input).unwrap_err(), InstanceUrlError::NotOrigin, "{input}");
        }
    }

    #[test]
    fn serializes_as_the_canonical_string_and_checks_ownership() {
        let parsed = InstanceOrigin::parse("http://127.0.0.1:4000").unwrap();
        let json = serde_json::to_string(&parsed).unwrap();
        assert_eq!(json, "\"http://127.0.0.1:4000\"");
        let back: InstanceOrigin = serde_json::from_str(&json).unwrap();
        assert_eq!(back, parsed);
        assert!(back.is_insecure());
        assert!(serde_json::from_str::<InstanceOrigin>("\"http://example.org\"").is_err());

        let official = InstanceOrigin::official();
        assert!(official.owns(&Url::parse("https://connect.discophile.app/oauth/token?x=1").unwrap()));
        assert!(official.owns(&Url::parse("https://CONNECT.discophile.app:443/oauth/token").unwrap()));
        for other in [
            "http://connect.discophile.app/oauth/token",
            "https://connect.discophile.app:8443/oauth/token",
            "https://evil.example/oauth/token",
            "https://user@connect.discophile.app/oauth/token",
            "https://connect.discophile.app/oauth/token#x",
        ] {
            assert!(!official.owns(&Url::parse(other).unwrap()), "{other}");
        }
    }
}
