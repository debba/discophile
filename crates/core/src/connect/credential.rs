//! Tokens of a Connect delegation. Tokens are opaque: they are stored as received (in the platform's
//! keychain, by the app) and never parsed. `Debug` output never contains them.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::instance::InstanceOrigin;

/// Access tokens expiring within this many milliseconds are refreshed first.
pub const REFRESH_MARGIN_MILLIS: i64 = 60_000;

/// A successful token response, with the expiry computed when it was received.
#[derive(Clone, PartialEq, Eq)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: String,
    /// Granted scopes; the requested ones when the server did not repeat them.
    pub scopes: Vec<String>,
    /// Unix milliseconds when the access token expires.
    pub expires_at: i64,
}

impl fmt::Debug for TokenSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenSet").field("scopes", &self.scopes).field("expires_at", &self.expires_at).finish_non_exhaustive()
    }
}

/// Where local data of a Connect account belongs: the instance's canonical origin and the account
/// identifier. An email address is never enough.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountNamespace {
    pub origin: InstanceOrigin,
    pub account_id: String,
}

impl AccountNamespace {
    /// A stable key for local storage, such as `connect/<key>`.
    pub fn key(&self) -> String {
        crate::md5_hex(format!("{}\n{}", self.origin, self.account_id).as_bytes())
    }
}

/// What the app keeps in secret storage after signing in: the tokens and the instance and account they
/// belong to.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectCredential {
    pub origin: InstanceOrigin,
    pub instance_id: String,
    pub account_id: String,
    pub client_id: String,
    pub access_token: String,
    pub refresh_token: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    /// Unix milliseconds when the access token expires.
    pub expires_at: i64,
}

impl fmt::Debug for ConnectCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectCredential")
            .field("origin", &self.origin)
            .field("instance_id", &self.instance_id)
            .field("account_id", &self.account_id)
            .field("client_id", &self.client_id)
            .field("scopes", &self.scopes)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

impl ConnectCredential {
    pub fn new(origin: InstanceOrigin, instance_id: String, account_id: String, client_id: String, tokens: TokenSet) -> Self {
        Self {
            origin,
            instance_id,
            account_id,
            client_id,
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            scopes: tokens.scopes,
            expires_at: tokens.expires_at,
        }
    }

    /// The same delegation with rotated tokens.
    pub fn rotated(&self, tokens: TokenSet) -> Self {
        Self::new(self.origin.clone(), self.instance_id.clone(), self.account_id.clone(), self.client_id.clone(), tokens)
    }

    /// Whether the access token expires within a minute (or already expired).
    pub fn needs_refresh(&self, now_millis: i64) -> bool {
        self.expires_at - now_millis <= REFRESH_MARGIN_MILLIS
    }

    pub fn namespace(&self) -> AccountNamespace {
        AccountNamespace { origin: self.origin.clone(), account_id: self.account_id.clone() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credential(expires_at: i64) -> ConnectCredential {
        let tokens = TokenSet {
            access_token: "dca_secret-access".into(),
            refresh_token: "dcr_secret-refresh".into(),
            scopes: vec!["account:read".into(), "offline_access".into()],
            expires_at,
        };
        ConnectCredential::new(InstanceOrigin::official(), "instance".into(), "u_1".into(), "discophile-desktop".into(), tokens)
    }

    #[test]
    fn refreshes_within_a_minute_of_expiry() {
        let now = 1_000_000;
        assert!(!credential(now + 61_000).needs_refresh(now));
        assert!(credential(now + 60_000).needs_refresh(now));
        assert!(credential(now - 1).needs_refresh(now));
    }

    #[test]
    fn debug_output_and_storage_format() {
        let credential = credential(42);
        let debug = format!("{credential:?}");
        assert!(!debug.contains("secret"), "{debug}");
        assert!(debug.contains("u_1"));
        let tokens = TokenSet { access_token: "secret".into(), refresh_token: "secret".into(), scopes: vec![], expires_at: 0 };
        assert!(!format!("{tokens:?}").contains("secret"));
        let json = serde_json::to_value(&credential).unwrap();
        assert_eq!(json["origin"], "https://connect.discophile.app");
        assert_eq!(json["instanceId"], "instance");
        assert_eq!(json["accountId"], "u_1");
        assert_eq!(json["expiresAt"], 42);
        let back: ConnectCredential = serde_json::from_value(json).unwrap();
        assert_eq!(back, credential);
        let rotated = credential.rotated(TokenSet { access_token: "a2".into(), refresh_token: "r2".into(), scopes: vec![], expires_at: 7 });
        assert_eq!(rotated.namespace(), credential.namespace());
        assert_eq!(rotated.refresh_token, "r2");
    }

    #[test]
    fn namespaces_combine_origin_and_account() {
        let a = credential(0).namespace();
        let mut b = a.clone();
        b.origin = InstanceOrigin::parse("https://connect.example.org").unwrap();
        assert_ne!(a.key(), b.key());
        let mut c = a.clone();
        c.account_id = "u_2".into();
        assert_ne!(a.key(), c.key());
        assert_eq!(a.key().len(), 32);
    }
}
