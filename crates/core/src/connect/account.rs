//! The account and its plan (`GET /api/v1/account`). Plans are informational: they never restrict local
//! playback or direct connections to the person's own servers, and an entitlement never enables a feature
//! by itself (the instance must advertise the capability and this client must implement it).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::discovery::ServerInfo;
use super::error::ConnectError;

const MAX_ID_CHARS: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    pub instance_id: String,
    pub account: Account,
    pub plan: Plan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// Stable and opaque, unique within the instance.
    pub id: String,
    /// Possibly empty.
    #[serde(default, deserialize_with = "string_or_empty")]
    pub name: String,
    /// Not a global identifier; it may change.
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub email_verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    /// The effective tier, already accounting for expiry.
    pub tier: Tier,
    /// English display name for tiers this client does not know.
    #[serde(default, deserialize_with = "string_or_empty")]
    pub name: String,
    #[serde(default)]
    pub source: PlanSource,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default, deserialize_with = "entitlements")]
    pub entitlements: Vec<Entitlement>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum Tier {
    Free,
    Connect,
    Supporter,
    /// A tier defined by the instance; shown with the plan's `name`.
    Other(String),
}

impl From<String> for Tier {
    fn from(value: String) -> Self {
        match value.as_str() {
            "free" => Self::Free,
            "connect" => Self::Connect,
            "supporter" => Self::Supporter,
            _ => Self::Other(value),
        }
    }
}

impl From<Tier> for String {
    fn from(value: Tier) -> Self {
        match value {
            Tier::Free => "free".into(),
            Tier::Connect => "connect".into(),
            Tier::Supporter => "supporter".into(),
            Tier::Other(id) => id,
        }
    }
}

/// How the tier was obtained.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum PlanSource {
    /// The default tier of every account.
    #[default]
    Default,
    /// Assigned by the instance's administrators.
    Manual,
    /// A source added later; shown generically.
    Other(String),
}

impl From<String> for PlanSource {
    fn from(value: String) -> Self {
        match value.as_str() {
            "default" => Self::Default,
            "manual" => Self::Manual,
            _ => Self::Other(value),
        }
    }
}

impl From<PlanSource> for String {
    fn from(value: PlanSource) -> Self {
        match value {
            PlanSource::Default => "default".into(),
            PlanSource::Manual => "manual".into(),
            PlanSource::Other(id) => id,
        }
    }
}

/// A benefit included in the tier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entitlement {
    pub id: String,
    /// A positive limit, such as a number of devices.
    #[serde(default)]
    pub limit: Option<u64>,
    /// Whether the instance offers the benefit now.
    #[serde(default)]
    pub available: bool,
}

/// The entitlements defined by protocol version 1. Other identifiers are ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntitlementKind {
    DevicesPaired,
    SyncConnections,
    SyncPreferences,
    BackupConfiguration,
    PlaybackRemote,
    PlaybackHandoff,
    SupporterRecognition,
    Previews,
}

impl EntitlementKind {
    pub const ALL: [Self; 8] = [
        Self::DevicesPaired,
        Self::SyncConnections,
        Self::SyncPreferences,
        Self::BackupConfiguration,
        Self::PlaybackRemote,
        Self::PlaybackHandoff,
        Self::SupporterRecognition,
        Self::Previews,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::DevicesPaired => "devices.paired",
            Self::SyncConnections => "sync.connections",
            Self::SyncPreferences => "sync.preferences",
            Self::BackupConfiguration => "backup.configuration",
            Self::PlaybackRemote => "playback.remote",
            Self::PlaybackHandoff => "playback.handoff",
            Self::SupporterRecognition => "supporter.recognition",
            Self::Previews => "previews",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.id() == id)
    }

    /// The capability an instance must advertise for this benefit to work, if any.
    pub fn capability(self) -> Option<&'static str> {
        match self {
            Self::DevicesPaired => Some("devices"),
            Self::SyncConnections | Self::SyncPreferences => Some("sync"),
            Self::BackupConfiguration => Some("backup"),
            Self::PlaybackRemote => Some("remote"),
            Self::PlaybackHandoff => Some("handoff"),
            Self::SupporterRecognition | Self::Previews => None,
        }
    }
}

/// How an entitlement is presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntitlementStatus {
    /// Not offered yet: `available` is false, or the instance does not advertise the capability.
    Planned,
    /// Offered by the instance, but this version of Discophile does not implement the capability.
    NotInThisVersion,
    /// Offered and usable (benefits without a capability, or capabilities this client implements).
    Included,
}

impl Entitlement {
    pub fn kind(&self) -> Option<EntitlementKind> {
        EntitlementKind::from_id(&self.id)
    }

    pub fn status(&self, info: &ServerInfo) -> EntitlementStatus {
        if !self.available {
            return EntitlementStatus::Planned;
        }
        match self.kind().and_then(EntitlementKind::capability) {
            None => EntitlementStatus::Included,
            Some(capability) if !info.capabilities.contains_key(capability) => EntitlementStatus::Planned,
            Some(capability) if info.capability_usable(capability) => EntitlementStatus::Included,
            Some(_) => EntitlementStatus::NotInThisVersion,
        }
    }
}

impl Plan {
    /// Entitlements this client knows, in server order and without repetitions.
    pub fn known_entitlements(&self) -> Vec<(EntitlementKind, &Entitlement)> {
        let mut known: Vec<(EntitlementKind, &Entitlement)> = Vec::new();
        for entitlement in &self.entitlements {
            if let Some(kind) = entitlement.kind()
                && !known.iter().any(|(k, _)| *k == kind)
            {
                known.push((kind, entitlement));
            }
        }
        known
    }
}

impl AccountInfo {
    /// Parses an account response and checks that it belongs to the instance described by `info`.
    pub fn parse(info: &ServerInfo, body: &[u8]) -> Result<Self, ConnectError> {
        let account: Self = serde_json::from_slice(body).map_err(|_| ConnectError::InvalidResponse)?;
        if account.instance_id != info.instance_id {
            return Err(ConnectError::InstanceMismatch);
        }
        let id = &account.account.id;
        if id.trim().is_empty() || id.chars().count() > MAX_ID_CHARS {
            return Err(ConnectError::InvalidResponse);
        }
        if matches!(&account.plan.tier, Tier::Other(tier) if tier.trim().is_empty()) {
            return Err(ConnectError::InvalidResponse);
        }
        Ok(account)
    }
}

fn string_or_empty<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

/// Entitlements without a string `id` are skipped; a limit that is not a positive integer is ignored.
fn entitlements<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Entitlement>, D::Error> {
    let values = Option::<Vec<Value>>::deserialize(deserializer)?.unwrap_or_default();
    Ok(values
        .iter()
        .filter_map(|value| {
            Some(Entitlement {
                id: value.get("id")?.as_str()?.to_string(),
                limit: value.get("limit").and_then(Value::as_u64).filter(|l| *l > 0),
                available: value.get("available").and_then(Value::as_bool).unwrap_or(false),
            })
        })
        .collect())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::connect::discovery::tests::document;
    use crate::connect::instance::InstanceOrigin;
    use serde_json::json;

    pub(crate) fn example() -> Value {
        json!({
            "instanceId": "9b0d0c5e-7f1e-4c39-9d0e-2f3a2c1b5e77",
            "account": {"id": "u_6sQb2kL8wN4tR1", "name": "Ada Example", "email": "ada@example.com", "emailVerified": true},
            "plan": {
                "tier": "connect",
                "name": "Connect",
                "source": "manual",
                "expiresAt": "2027-01-31T00:00:00Z",
                "entitlements": [
                    {"id": "devices.paired", "limit": 10, "available": false},
                    {"id": "sync.connections", "available": false},
                    {"id": "sync.preferences", "available": false},
                    {"id": "backup.configuration", "available": false},
                    {"id": "playback.remote", "available": false},
                    {"id": "playback.handoff", "available": false}
                ]
            }
        })
    }

    fn info(capabilities: Value) -> ServerInfo {
        let origin = InstanceOrigin::official();
        let mut value = document(origin.as_str());
        value["capabilities"] = capabilities;
        ServerInfo::parse(&origin, value.to_string().as_bytes()).unwrap()
    }

    fn parse(value: &Value) -> Result<AccountInfo, ConnectError> {
        AccountInfo::parse(&info(json!({"account": {"version": 1}})), value.to_string().as_bytes())
    }

    #[test]
    fn parses_the_protocol_example() {
        let account = parse(&example()).unwrap();
        assert_eq!(account.account.id, "u_6sQb2kL8wN4tR1");
        assert_eq!(account.account.email.as_deref(), Some("ada@example.com"));
        assert!(account.account.email_verified);
        assert_eq!(account.plan.tier, Tier::Connect);
        assert_eq!(account.plan.source, PlanSource::Manual);
        assert_eq!(account.plan.expires_at.unwrap().to_rfc3339(), "2027-01-31T00:00:00+00:00");
        let mut fractional = example();
        fractional["plan"]["expiresAt"] = json!("2027-03-01T12:00:00.000Z");
        let expires = parse(&fractional).unwrap().plan.expires_at.unwrap();
        assert_eq!(expires.timestamp(), 1_803_902_400);
        let known = account.plan.known_entitlements();
        assert_eq!(known.len(), 6);
        assert_eq!(known[0].0, EntitlementKind::DevicesPaired);
        assert_eq!(known[0].1.limit, Some(10));
        let info = info(json!({"account": {"version": 1}}));
        assert!(known.iter().all(|(_, e)| e.status(&info) == EntitlementStatus::Planned));
    }

    #[test]
    fn handles_unknown_tiers_sources_entitlements_and_null_members() {
        let mut value = example();
        value["account"]["name"] = Value::Null;
        value["account"]["email"] = Value::Null;
        value["account"]["future"] = json!(1);
        value["plan"] = json!({
            "tier": "patron",
            "name": "Patron",
            "source": "subscription",
            "expiresAt": null,
            "entitlements": [
                {"id": "future.thing", "available": true},
                {"id": "previews", "available": true, "limit": 0},
                {"id": "previews", "available": false},
                {"id": "supporter.recognition"},
                {"limit": 3},
                "not an object"
            ]
        });
        let account = parse(&value).unwrap();
        assert_eq!(account.account.name, "");
        assert!(account.account.email.is_none());
        assert_eq!(account.plan.tier, Tier::Other("patron".into()));
        assert_eq!(account.plan.name, "Patron");
        assert_eq!(account.plan.source, PlanSource::Other("subscription".into()));
        assert!(account.plan.expires_at.is_none());
        assert_eq!(account.plan.entitlements.len(), 4);
        let known = account.plan.known_entitlements();
        assert_eq!(known.iter().map(|(k, _)| *k).collect::<Vec<_>>(), vec![EntitlementKind::Previews, EntitlementKind::SupporterRecognition]);
        assert_eq!(known[0].1.limit, None);
        let info = info(json!({"account": {"version": 1}}));
        assert_eq!(known[0].1.status(&info), EntitlementStatus::Included);
        assert_eq!(known[1].1.status(&info), EntitlementStatus::Planned);
        assert_eq!(String::from(Tier::Supporter), "supporter");
        assert_eq!(String::from(PlanSource::Default), "default");
    }

    #[test]
    fn available_needs_the_capability_and_this_client() {
        let entitlement = Entitlement { id: "sync.connections".into(), limit: None, available: true };
        assert_eq!(entitlement.status(&info(json!({"account": {"version": 1}}))), EntitlementStatus::Planned);
        assert_eq!(entitlement.status(&info(json!({"account": {"version": 1}, "sync": {"version": 1}}))), EntitlementStatus::NotInThisVersion);
        let unavailable = Entitlement { available: false, ..entitlement };
        assert_eq!(unavailable.status(&info(json!({"sync": {"version": 1}}))), EntitlementStatus::Planned);
    }

    #[test]
    fn rejects_other_instances_and_invalid_identifiers() {
        let mut other = example();
        other["instanceId"] = json!("another-instance");
        assert_eq!(parse(&other).unwrap_err(), ConnectError::InstanceMismatch);
        let mut empty = example();
        empty["account"]["id"] = json!("");
        assert_eq!(parse(&empty).unwrap_err(), ConnectError::InvalidResponse);
        let mut long = example();
        long["account"]["id"] = json!("u".repeat(129));
        assert_eq!(parse(&long).unwrap_err(), ConnectError::InvalidResponse);
        let mut no_tier = example();
        no_tier["plan"]["tier"] = json!("");
        assert_eq!(parse(&no_tier).unwrap_err(), ConnectError::InvalidResponse);
        let mut bad_date = example();
        bad_date["plan"]["expiresAt"] = json!("tomorrow");
        assert_eq!(parse(&bad_date).unwrap_err(), ConnectError::InvalidResponse);
        assert_eq!(parse(&json!({"error": "invalid_token"})).unwrap_err(), ConnectError::InvalidResponse);
    }
}
