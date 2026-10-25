use super::{Plan, vault::Envelope};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultInfo {
    pub id: Uuid,
    pub epoch: u32,
    pub revision: u64,
    pub rotation_required: bool,
    pub connection_count: usize,
    pub envelope: Option<Envelope>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedDevice {
    pub id: Uuid,
    pub name: String,
    pub online: bool,
    pub suspended: bool,
    pub state: Option<Envelope>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    pub id: Uuid,
    pub epoch: u32,
    pub revision: u64,
    pub created_at: String,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: Uuid,
    pub sender_id: Uuid,
    pub kind: String,
    pub envelope: Envelope,
    pub expires_at: String,
}
#[derive(Deserialize)]
pub struct Poll {
    pub vault: VaultInfo,
    pub plan: Plan,
    pub devices: Vec<PairedDevice>,
    pub messages: Vec<Message>,
    pub backups: Vec<Backup>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub version: u32,
    pub currency: String,
    pub activation: String,
    pub contact: String,
    pub tiers: Vec<CatalogTier>,
    pub backup_retention_days: u32,
    pub max_backups: u32,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogTier {
    pub id: String,
    pub name: String,
    pub monthly_cents: u32,
    pub yearly_cents: u32,
    pub entitlements: Vec<super::Entitlement>,
}
