//! Instance discovery: `GET /api/v1/server-info`, checked against the origin the client used. Every
//! endpoint and resource must belong to that origin; unknown members and capabilities are ignored.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use super::error::ConnectError;
use super::instance::InstanceOrigin;

pub const SERVER_INFO_PATH: &str = "/api/v1/server-info";
pub const PROTOCOL: &str = "discophile-connect";
/// The protocol version this client implements.
pub const SUPPORTED_VERSION: u64 = 1;
/// `GET /api/v1/account`, version 1.
pub const ACCOUNT_CAPABILITY: &str = "account";
pub const ACCOUNT_CAPABILITY_VERSION: u64 = 1;
pub const ACCOUNT_SCOPE: &str = "account:read";
pub const OFFLINE_SCOPE: &str = "offline_access";
/// The native client identifier of Discophile for computers.
pub const DESKTOP_CLIENT_ID: &str = "discophile-desktop";
/// Capabilities, with their versions, that this version of Discophile implements.
pub const IMPLEMENTED_CAPABILITIES: &[(&str, u64)] = &[(ACCOUNT_CAPABILITY, ACCOUNT_CAPABILITY_VERSION), ("devices", 1), ("sync", 1), ("account-sync", 1), ("backup", 1), ("remote", 1), ("handoff", 1)];
const MAX_ID_CHARS: usize = 128;
const MAX_NAME_CHARS: usize = 100;

/// A validated discovery document. It can be cached (it holds no secrets) and is re-checked on load with
/// [`ServerInfo::is_consistent`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub origin: InstanceOrigin,
    pub instance_id: String,
    /// Display name, at most 100 characters. Always shown together with the origin.
    #[serde(default)]
    pub name: Option<String>,
    pub issuer: String,
    pub authorization_endpoint: Url,
    pub token_endpoint: Url,
    pub revocation_endpoint: Url,
    /// `resources.api` (always `<origin>/api/v1`), sent as the OAuth `resource` indicator.
    pub resource: String,
    pub scopes_supported: Vec<String>,
    pub native_clients: Vec<String>,
    /// Capabilities offered now, by name, with the version implemented.
    pub capabilities: BTreeMap<String, u64>,
    /// Account management in a browser; dropped when it points to another origin.
    #[serde(default)]
    pub dashboard: Option<Url>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Document {
    protocol: String,
    protocol_versions: Vec<Value>,
    instance: InstanceDocument,
    authorization: AuthorizationDocument,
    resources: ResourcesDocument,
    capabilities: BTreeMap<String, Value>,
    #[serde(default)]
    links: Option<LinksDocument>,
}

#[derive(Deserialize)]
struct InstanceDocument {
    id: String,
    origin: String,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthorizationDocument {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    revocation_endpoint: String,
    scopes_supported: Vec<String>,
    native_clients: Vec<String>,
}

#[derive(Deserialize)]
struct ResourcesDocument {
    api: String,
    /// Other resources (for example an administration endpoint): they must belong to the origin too.
    #[serde(flatten)]
    others: BTreeMap<String, Value>,
}

#[derive(Deserialize)]
struct LinksDocument {
    #[serde(default)]
    dashboard: Option<Value>,
}

impl ServerInfo {
    /// Parses and validates a discovery document fetched from `origin`.
    pub fn parse(origin: &InstanceOrigin, body: &[u8]) -> Result<Self, ConnectError> {
        let document: Document = serde_json::from_slice(body).map_err(|_| ConnectError::InvalidResponse)?;
        if document.protocol != PROTOCOL || !document.protocol_versions.iter().any(|v| v.as_u64() == Some(SUPPORTED_VERSION)) {
            return Err(ConnectError::Unsupported);
        }
        let id = document.instance.id;
        if id.trim().is_empty() || id.chars().count() > MAX_ID_CHARS {
            return Err(ConnectError::InvalidResponse);
        }
        if document.instance.origin != origin.as_str() || document.authorization.issuer != origin.as_str() {
            return Err(ConnectError::OriginMismatch);
        }
        let endpoint = |text: &str| -> Result<Url, ConnectError> {
            let url = Url::parse(text).map_err(|_| ConnectError::InvalidResponse)?;
            if url.query().is_some() || url.fragment().is_some() {
                return Err(ConnectError::InvalidResponse);
            }
            if origin.owns(&url) { Ok(url) } else { Err(ConnectError::OriginMismatch) }
        };
        let authorization = &document.authorization;
        let authorization_endpoint = endpoint(&authorization.authorization_endpoint)?;
        let token_endpoint = endpoint(&authorization.token_endpoint)?;
        let revocation_endpoint = endpoint(&authorization.revocation_endpoint)?;
        endpoint(&document.resources.api)?;
        for other in document.resources.others.values().filter_map(Value::as_str) {
            endpoint(other)?;
        }
        if document.resources.api != api_resource(origin) {
            return Err(ConnectError::InvalidResponse);
        }
        if ![ACCOUNT_SCOPE, OFFLINE_SCOPE].iter().all(|scope| authorization.scopes_supported.iter().any(|s| s == scope)) {
            return Err(ConnectError::NotOffered);
        }
        let capabilities = document
            .capabilities
            .iter()
            .filter_map(|(name, value)| Some((name.clone(), value.get("version")?.as_u64()?)))
            .collect();
        let dashboard = document
            .links
            .and_then(|l| l.dashboard)
            .and_then(|v| v.as_str().and_then(|s| Url::parse(s).ok()))
            .filter(|url| is_page_of(origin, url));
        Ok(Self {
            origin: origin.clone(),
            instance_id: id,
            name: document.instance.name.as_deref().and_then(display_name),
            issuer: document.authorization.issuer,
            authorization_endpoint,
            token_endpoint,
            revocation_endpoint,
            resource: document.resources.api,
            scopes_supported: document.authorization.scopes_supported,
            native_clients: document.authorization.native_clients,
            capabilities,
            dashboard,
        })
    }

    /// Whether every URL still belongs to the origin (for documents read back from storage).
    pub fn is_consistent(&self) -> bool {
        let endpoint = |url: &Url| self.origin.owns(url) && url.query().is_none();
        self.issuer == self.origin.as_str()
            && endpoint(&self.authorization_endpoint)
            && endpoint(&self.token_endpoint)
            && endpoint(&self.revocation_endpoint)
            && self.resource == api_resource(&self.origin)
            && self.dashboard.as_ref().is_none_or(|url| is_page_of(&self.origin, url))
    }

    /// Whether the instance advertises `name` at `version` or later. A capability's version only grows
    /// with additions that older clients can ignore.
    pub fn has_capability(&self, name: &str, version: u64) -> bool {
        self.capabilities.get(name).is_some_and(|advertised| *advertised >= version)
    }

    /// Whether both the instance and this version of Discophile implement the capability.
    pub fn capability_usable(&self, name: &str) -> bool {
        IMPLEMENTED_CAPABILITIES.iter().any(|(implemented, version)| *implemented == name && self.has_capability(name, *version))
    }

    /// Checks that Discophile for computers can sign in and read the account here.
    pub fn check_desktop_sign_in(&self) -> Result<(), ConnectError> {
        if self.capability_usable(ACCOUNT_CAPABILITY) && self.native_clients.iter().any(|c| c == DESKTOP_CLIENT_ID) {
            Ok(())
        } else {
            Err(ConnectError::NotOffered)
        }
    }

    /// `GET /api/v1/account` on this instance.
    pub fn account_url(&self) -> Url {
        self.origin.url("/api/v1/account")
    }
}

/// A browser page of the instance: same origin, no user name or password.
fn is_page_of(origin: &InstanceOrigin, url: &Url) -> bool {
    origin.same_origin(url) && url.username().is_empty() && url.password().is_none()
}

/// The resource indicator of version 1: `<origin>/api/v1`.
fn api_resource(origin: &InstanceOrigin) -> String {
    format!("{origin}/api/v1")
}

/// A display name without control characters, at most 100 characters; `None` when empty.
fn display_name(name: &str) -> Option<String> {
    let clean: String = name.chars().filter(|c| !c.is_control()).collect();
    let clean: String = clean.trim().chars().take(MAX_NAME_CHARS).collect();
    let clean = clean.trim_end().to_string();
    (!clean.is_empty()).then_some(clean)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    /// The discovery example of the protocol, for `origin`.
    pub(crate) fn document(origin: &str) -> Value {
        json!({
            "protocol": "discophile-connect",
            "protocolVersions": [1],
            "instance": {"id": "9b0d0c5e-7f1e-4c39-9d0e-2f3a2c1b5e77", "origin": origin, "name": "Discophile Connect"},
            "authorization": {
                "issuer": origin,
                "authorizationEndpoint": format!("{origin}/oauth/authorize"),
                "tokenEndpoint": format!("{origin}/oauth/token"),
                "revocationEndpoint": format!("{origin}/oauth/revoke"),
                "scopesSupported": ["account:read", "offline_access"],
                "nativeClients": ["discophile-desktop", "discophile-ios"]
            },
            "resources": {"api": format!("{origin}/api/v1"), "mcp": format!("{origin}/mcp")},
            "capabilities": {"account": {"version": 1}},
            "links": {"dashboard": format!("{origin}/account")}
        })
    }

    fn parse(origin: &str, value: &Value) -> Result<ServerInfo, ConnectError> {
        ServerInfo::parse(&InstanceOrigin::parse(origin).unwrap(), value.to_string().as_bytes())
    }

    fn with(origin: &str, pointer: &str, replacement: Value) -> Value {
        let mut value = document(origin);
        *value.pointer_mut(pointer).unwrap() = replacement;
        value
    }

    const OFFICIAL: &str = "https://connect.discophile.app";

    #[test]
    fn accepts_the_protocol_example_and_ignores_unknown_members() {
        let mut value = document(OFFICIAL);
        value["future"] = json!({"anything": true});
        value["capabilities"]["sync"] = json!({"version": 3});
        value["capabilities"]["odd"] = json!("not an object");
        value["protocolVersions"] = json!([1, 2]);
        let info = parse(OFFICIAL, &value).unwrap();
        assert_eq!(info.instance_id, "9b0d0c5e-7f1e-4c39-9d0e-2f3a2c1b5e77");
        assert_eq!(info.name.as_deref(), Some("Discophile Connect"));
        assert_eq!(info.token_endpoint.as_str(), "https://connect.discophile.app/oauth/token");
        assert_eq!(info.resource, "https://connect.discophile.app/api/v1");
        assert_eq!(info.dashboard.as_ref().map(Url::as_str), Some("https://connect.discophile.app/account"));
        assert!(info.capability_usable("account"));
        assert!(info.has_capability("sync", 3) && info.has_capability("sync", 1) && !info.has_capability("sync", 4));
        assert!(info.capability_usable("sync"));
        assert!(!info.capabilities.contains_key("odd"));
        assert_eq!(info.account_url().as_str(), "https://connect.discophile.app/api/v1/account");
        assert!(info.check_desktop_sign_in().is_ok());
        assert!(info.is_consistent());
        let cached: ServerInfo = serde_json::from_str(&serde_json::to_string(&info).unwrap()).unwrap();
        assert_eq!(cached, info);
    }

    #[test]
    fn rejects_other_protocols_and_versions() {
        assert_eq!(parse(OFFICIAL, &with(OFFICIAL, "/protocol", json!("other"))).unwrap_err(), ConnectError::Unsupported);
        assert_eq!(parse(OFFICIAL, &with(OFFICIAL, "/protocolVersions", json!([2]))).unwrap_err(), ConnectError::Unsupported);
        assert_eq!(parse(OFFICIAL, &with(OFFICIAL, "/protocolVersions", json!([]))).unwrap_err(), ConnectError::Unsupported);
        assert_eq!(parse(OFFICIAL, &json!({"protocol": "discophile-connect"})).unwrap_err(), ConnectError::InvalidResponse);
        assert_eq!(ServerInfo::parse(&InstanceOrigin::official(), b"<html>").unwrap_err(), ConnectError::InvalidResponse);
    }

    #[test]
    fn binds_every_url_to_the_origin_used() {
        // A document copied from another instance.
        assert_eq!(parse("https://connect.example.org", &document(OFFICIAL)).unwrap_err(), ConnectError::OriginMismatch);
        for (pointer, value) in [
            ("/instance/origin", json!("https://connect.discophile.app/")),
            ("/authorization/issuer", json!("https://issuer.example")),
            ("/authorization/authorizationEndpoint", json!("https://evil.example/oauth/authorize")),
            ("/authorization/tokenEndpoint", json!("http://connect.discophile.app/oauth/token")),
            ("/authorization/tokenEndpoint", json!("https://connect.discophile.app:8443/oauth/token")),
            ("/authorization/revocationEndpoint", json!("https://user@connect.discophile.app/oauth/revoke")),
            ("/resources/api", json!("http://connect.discophile.app/api/v1")),
            ("/resources/mcp", json!("https://elsewhere.example/mcp")),
            ("/resources/api", json!("https://api.example/api/v1")),
        ] {
            assert_eq!(parse(OFFICIAL, &with(OFFICIAL, pointer, value)).unwrap_err(), ConnectError::OriginMismatch, "{pointer}");
        }
        assert_eq!(
            parse(OFFICIAL, &with(OFFICIAL, "/authorization/tokenEndpoint", json!("/oauth/token"))).unwrap_err(),
            ConnectError::InvalidResponse
        );
        for (pointer, value) in [
            ("/resources/api", json!("https://connect.discophile.app/api/v2")),
            ("/resources/api", json!("https://connect.discophile.app/api/v1/")),
            ("/resources/api", json!("https://connect.discophile.app/api/v1?x=1")),
            ("/authorization/tokenEndpoint", json!("https://connect.discophile.app/oauth/token?tenant=a")),
            ("/authorization/authorizationEndpoint", json!("https://connect.discophile.app/oauth/authorize#x")),
            ("/authorization/revocationEndpoint", json!("https://connect.discophile.app/oauth/revoke?")),
        ] {
            assert_eq!(parse(OFFICIAL, &with(OFFICIAL, pointer, value)).unwrap_err(), ConnectError::InvalidResponse, "{pointer}");
        }
        // A dashboard elsewhere is not offered, but the document stays usable.
        let info = parse(OFFICIAL, &with(OFFICIAL, "/links/dashboard", json!("https://elsewhere.example/account"))).unwrap();
        assert!(info.dashboard.is_none());
        let anchored = parse(OFFICIAL, &with(OFFICIAL, "/links/dashboard", json!("https://connect.discophile.app/account#plan"))).unwrap();
        assert_eq!(anchored.dashboard.as_ref().map(Url::as_str), Some("https://connect.discophile.app/account#plan"));
        assert!(anchored.is_consistent());
        let mut tampered = info.clone();
        tampered.token_endpoint = Url::parse("https://evil.example/token").unwrap();
        assert!(!tampered.is_consistent());
    }

    #[test]
    fn checks_identifiers_names_scopes_and_clients() {
        assert_eq!(parse(OFFICIAL, &with(OFFICIAL, "/instance/id", json!(""))).unwrap_err(), ConnectError::InvalidResponse);
        assert_eq!(parse(OFFICIAL, &with(OFFICIAL, "/instance/id", json!("x".repeat(129)))).unwrap_err(), ConnectError::InvalidResponse);
        assert!(parse(OFFICIAL, &with(OFFICIAL, "/instance/id", json!("x".repeat(128)))).is_ok());
        let long = parse(OFFICIAL, &with(OFFICIAL, "/instance/name", json!(format!("\u{7}{}", "n".repeat(150))))).unwrap();
        assert_eq!(long.name.unwrap().chars().count(), 100);
        assert!(parse(OFFICIAL, &with(OFFICIAL, "/instance/name", json!("  "))).unwrap().name.is_none());
        for scopes in [json!(["offline_access"]), json!(["account:read"])] {
            assert_eq!(parse(OFFICIAL, &with(OFFICIAL, "/authorization/scopesSupported", scopes)).unwrap_err(), ConnectError::NotOffered);
        }
        let ios_only = parse(OFFICIAL, &with(OFFICIAL, "/authorization/nativeClients", json!(["discophile-ios"]))).unwrap();
        assert_eq!(ios_only.check_desktop_sign_in().unwrap_err(), ConnectError::NotOffered);
        let no_account = parse(OFFICIAL, &with(OFFICIAL, "/capabilities", json!({}))).unwrap();
        assert_eq!(no_account.check_desktop_sign_in().unwrap_err(), ConnectError::NotOffered);
        // Capability versions only grow compatibly.
        let account_v2 = parse(OFFICIAL, &with(OFFICIAL, "/capabilities", json!({"account": {"version": 2}}))).unwrap();
        assert!(account_v2.capability_usable("account"));
        assert!(account_v2.check_desktop_sign_in().is_ok());
        let account_v0 = parse(OFFICIAL, &with(OFFICIAL, "/capabilities", json!({"account": {"version": 0}}))).unwrap();
        assert!(!account_v0.capability_usable("account"));
        let insecure = parse("http://localhost:3000", &document("http://localhost:3000")).unwrap();
        assert!(insecure.origin.is_insecure());
    }
}
