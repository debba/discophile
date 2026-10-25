//! Local integration check launched by connect-app/scripts/verify-desktop-services.ts.
use discophile_core::connect::{ConnectClient, ConnectCredential, InstanceOrigin, TokenSet};
use discophile_core::connect::vault::{Envelope, VaultKeys};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let origin = InstanceOrigin::parse(&std::env::var("CONNECT_SMOKE_ORIGIN")?)?;
    if !origin.is_insecure() { return Err("Local development only".into()); }
    let client = ConnectClient::new();
    let info = client.server_info(&origin).await?;
    let owner = std::env::var("CONNECT_SMOKE_ACCOUNT")?;
    let credential = |name: &str| -> Result<ConnectCredential, std::env::VarError> {
        Ok(ConnectCredential::new(origin.clone(), info.instance_id.clone(), owner.clone(), "discophile-desktop".into(),
            TokenSet { access_token: std::env::var(name)?, refresh_token: "unused-local-test".into(),
                scopes: vec!["account:read".into(), "connect:manage".into()], expires_at: i64::MAX }))
    };
    let a = credential("CONNECT_SMOKE_TOKEN_A")?;
    let b = credential("CONNECT_SMOKE_TOKEN_B")?;
    assert_eq!(client.account_for(&info, &a).await?.account.id, owner);
    assert_eq!(client.account_for(&info, &b).await?.account.id, owner);
    let mut keys = VaultKeys::new()?;
    let old_keys = keys.clone();
    let id_a = Uuid::new_v4(); let id_b = Uuid::new_v4();
    let secret_a = discophile_core::oauth::random_token().unwrap();
    let secret_b = discophile_core::oauth::random_token().unwrap();
    let connection = discophile_core::ServerAccount::new(discophile_core::Credentials::simple(
        "https://music.example.invalid", "fixture", "encrypted-fixture-only", false, discophile_core::MusicProvider::Subsonic)?, "Smoke fixture");
    let configuration = json!({"version":1,"connections":[{"kind":"music","account":connection}],"preferences":null});
    let initial = keys.seal(&info.instance_id, id_a, "configuration", 1, &configuration)?;
    client.service(&info, &a, &json!({"action":"join","deviceId":id_a,"deviceSecret":secret_a,"name":"Smoke A",
        "vaultId":keys.vault_id,"epoch":1,"envelope":initial,"connectionCount":1})).await?;
    let recovered = VaultKeys::recover(&keys.recovery_code())?;
    client.service(&info, &b, &json!({"action":"join","deviceId":id_b,"deviceSecret":secret_b,"name":"Smoke B",
        "vaultId":keys.vault_id,"epoch":1,"connectionCount":0})).await?;
    let update = keys.seal(&info.instance_id, id_a, "configuration", 2, &configuration)?;
    client.service(&info, &a, &json!({"action":"write","deviceId":id_a,"expectedRevision":1,"connectionCount":1,"envelope":update})).await?;
    let polled = client.service(&info, &b, &json!({"action":"poll","deviceId":id_b})).await?;
    let envelope: Envelope = serde_json::from_value(polled["vault"]["envelope"].clone())?;
    assert_eq!(recovered.open::<Value>(&envelope, &info.instance_id, "configuration")?, configuration);
    assert_eq!(polled["devices"].as_array().unwrap().len(), 2);
    println!("PASS: two native clients pair and synchronize authenticated encrypted configuration.");
    client.service(&info, &a, &json!({"action":"backup","deviceId":id_a})).await?;
    let polled = client.service(&info, &a, &json!({"action":"poll","deviceId":id_a})).await?;
    let backup_id = polled["backups"][0]["id"].clone();
    let backup = client.service(&info, &a, &json!({"action":"restore","deviceId":id_a,"id":backup_id})).await?;
    let restored: Envelope = serde_json::from_value(backup["envelope"].clone())?;
    assert_eq!(keys.open::<Value>(&restored, &info.instance_id, "configuration")?, configuration);
    println!("PASS: backup and restore preserve the native encrypted configuration.");
    let message_id = Uuid::new_v4();
    let object = format!("message:{id_b}:{message_id}:remote");
    let body = json!({"version":1,"createdAt":discophile_core::now_millis(),"replyTo":null,"message":{"message":"command","target":id_b,"command":{"command":"pause"}}});
    let command = keys.seal(&info.instance_id, id_a, &object, 1, &body)?;
    client.service(&info, &a, &json!({"action":"send","deviceId":id_a,"id":message_id,"targetId":id_b,"kind":"remote","envelope":command})).await?;
    let inbox = client.service(&info, &b, &json!({"action":"poll","deviceId":id_b})).await?;
    let received: Envelope = serde_json::from_value(inbox["messages"][0]["envelope"].clone())?;
    assert_eq!(recovered.open::<Value>(&received, &info.instance_id, &object)?, body);
    client.service(&info, &b, &json!({"action":"ack","deviceId":id_b,"ids":[message_id]})).await?;
    println!("PASS: native encrypted remote commands are delivered and acknowledged.");
    client.service(&info, &a, &json!({"action":"revoke","deviceId":id_a,"targetId":id_b})).await?;
    keys.rotate()?;
    let rotated = keys.seal(&info.instance_id, id_a, "configuration", 3, &configuration)?;
    assert!(old_keys.open::<Value>(&rotated, &info.instance_id, "configuration").is_err());
    client.service(&info, &a, &json!({"action":"rotate","deviceId":id_a,"expectedRevision":2,"connectionCount":1,"envelope":rotated})).await?;
    assert!(client.service(&info, &b, &json!({"action":"poll","deviceId":id_b})).await.is_err());
    assert_eq!(keys.open::<Value>(&restored, &info.instance_id, "configuration")?, configuration);
    client.service(&info, &a, &json!({"action":"delete","deviceId":id_a})).await?;
    let empty = client.service(&info, &a, &json!({"action":"inspect"})).await?;
    assert!(empty["vault"].is_null());
    println!("PASS: revocation, key rotation, historical backup recovery and cloud deletion.");
    Ok(())
}
