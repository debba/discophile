//! Native vault suite and personal recovery. See connect/protocol/services.md for the wire contract.

use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::BTreeMap;
use uuid::Uuid;
use zeroize::Zeroize;

use super::ConnectError;

pub const MAX_PLAINTEXT: usize = 250_000;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Envelope {
    pub version: u32,
    pub instance_id: String,
    pub vault_id: Uuid,
    pub epoch: u32,
    pub sender_id: Uuid,
    pub object_id: String,
    pub revision: u64,
    pub nonce: String,
    pub ciphertext: String,
}

impl Envelope {
    pub fn aad(&self) -> Vec<u8> {
        serde_json::to_vec(&(
            "discophile-connect-vault",
            self.version,
            &self.instance_id,
            self.vault_id,
            self.epoch,
            self.sender_id,
            &self.object_id,
            self.revision,
        ))
        .expect("serializable envelope context")
    }
    pub fn fingerprint(&self) -> String {
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(
            serde_json::to_vec(self).expect("serializable envelope"),
        ))
    }
}

/// Deliberately has no Debug implementation: keys must not enter logs or diagnostic dumps.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VaultKeys {
    pub vault_id: Uuid,
    pub epoch: u32,
    keys: BTreeMap<u32, String>,
}

impl Drop for VaultKeys {
    fn drop(&mut self) {
        for key in self.keys.values_mut() {
            key.zeroize();
        }
    }
}

impl VaultKeys {
    pub fn new() -> Result<Self, ConnectError> {
        let mut value = Self {
            vault_id: Uuid::new_v4(),
            epoch: 0,
            keys: BTreeMap::new(),
        };
        value.rotate()?;
        Ok(value)
    }
    pub fn rotate(&mut self) -> Result<(), ConnectError> {
        if self.keys.len() >= 128 {
            return Err(ConnectError::Unavailable);
        }
        let mut key = [0u8; 32];
        getrandom::fill(&mut key).map_err(|_| ConnectError::Unavailable)?;
        self.epoch = self.epoch.checked_add(1).ok_or(ConnectError::Unavailable)?;
        self.keys.insert(self.epoch, B64.encode(key));
        key.zeroize();
        Ok(())
    }
    pub fn recovery_code(&self) -> String {
        format!(
            "dc1_{}",
            B64.encode(serde_json::to_vec(self).expect("serializable recovery"))
        )
    }
    pub fn recover(code: &str) -> Result<Self, ConnectError> {
        if code.len() > 16_384 {
            return Err(ConnectError::InvalidResponse);
        }
        let bytes = B64
            .decode(
                code.trim()
                    .strip_prefix("dc1_")
                    .ok_or(ConnectError::InvalidResponse)?,
            )
            .map_err(|_| ConnectError::InvalidResponse)?;
        let keys: Self =
            serde_json::from_slice(&bytes).map_err(|_| ConnectError::InvalidResponse)?;
        if keys.epoch == 0
            || keys.keys.is_empty()
            || keys.keys.len() > 128
            || !keys.keys.contains_key(&keys.epoch)
            || keys.keys.iter().any(|(epoch, key)| {
                *epoch == 0
                    || *epoch > keys.epoch
                    || B64.decode(key).map_or(true, |key| key.len() != 32)
            })
        {
            return Err(ConnectError::InvalidResponse);
        }
        Ok(keys)
    }
    fn cipher(&self, epoch: u32) -> Result<Aes256Gcm, ConnectError> {
        let mut key = B64
            .decode(self.keys.get(&epoch).ok_or(ConnectError::Unavailable)?)
            .map_err(|_| ConnectError::InvalidResponse)?;
        let result = Aes256Gcm::new_from_slice(&key).map_err(|_| ConnectError::InvalidResponse);
        key.zeroize();
        result
    }
    pub fn seal<T: Serialize>(
        &self,
        instance: &str,
        sender: Uuid,
        object: &str,
        revision: u64,
        value: &T,
    ) -> Result<Envelope, ConnectError> {
        let mut plain = serde_json::to_vec(value).map_err(|_| ConnectError::InvalidResponse)?;
        if plain.len() > MAX_PLAINTEXT {
            plain.zeroize();
            return Err(ConnectError::Unavailable);
        }
        let mut nonce = [0u8; 12];
        getrandom::fill(&mut nonce).map_err(|_| ConnectError::Unavailable)?;
        let mut envelope = Envelope {
            version: 1,
            instance_id: instance.into(),
            vault_id: self.vault_id,
            epoch: self.epoch,
            sender_id: sender,
            object_id: object.into(),
            revision,
            nonce: B64.encode(nonce),
            ciphertext: String::new(),
        };
        let encrypted = self.cipher(self.epoch)?.encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &plain,
                aad: &envelope.aad(),
            },
        );
        plain.zeroize();
        envelope.ciphertext = B64.encode(encrypted.map_err(|_| ConnectError::InvalidResponse)?);
        Ok(envelope)
    }
    pub fn open<T: DeserializeOwned>(
        &self,
        envelope: &Envelope,
        instance: &str,
        object: &str,
    ) -> Result<T, ConnectError> {
        if envelope.version != 1
            || envelope.instance_id != instance
            || envelope.vault_id != self.vault_id
            || envelope.object_id != object
            || envelope.epoch > self.epoch
            || envelope.revision == 0
            || envelope.ciphertext.len() > 350_000
        {
            return Err(ConnectError::InvalidResponse);
        }
        let nonce = B64
            .decode(&envelope.nonce)
            .map_err(|_| ConnectError::InvalidResponse)?;
        if nonce.len() != 12 {
            return Err(ConnectError::InvalidResponse);
        }
        let ciphertext = B64
            .decode(&envelope.ciphertext)
            .map_err(|_| ConnectError::InvalidResponse)?;
        let mut plain = self
            .cipher(envelope.epoch)?
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: &envelope.aad(),
                },
            )
            .map_err(|_| ConnectError::InvalidResponse)?;
        let value = serde_json::from_slice(&plain).map_err(|_| ConnectError::InvalidResponse);
        plain.zeroize();
        value
    }
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Watermark {
    pub epoch: u32,
    pub revision: u64,
    pub fingerprint: String,
}
impl Watermark {
    pub fn observe(&mut self, envelope: &Envelope) -> Result<(), ConnectError> {
        if envelope.epoch < self.epoch
            || envelope.revision < self.revision
            || (envelope.revision == self.revision
                && !self.fingerprint.is_empty()
                && self.fingerprint != envelope.fingerprint())
        {
            return Err(ConnectError::InvalidResponse);
        }
        *self = Self {
            epoch: envelope.epoch,
            revision: envelope.revision,
            fingerprint: envelope.fingerprint(),
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn interoperates_with_cryptokit_and_node() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("testdata/vault-v1.json")).unwrap();
        let bundle = format!(
            "dc1_{}",
            B64.encode(serde_json::to_vec(&fixture["keys"]).unwrap())
        );
        let keys = VaultKeys::recover(&bundle).unwrap();
        let envelope: Envelope = serde_json::from_value(fixture["envelope"].clone()).unwrap();
        let plain = keys
            .open::<serde_json::Value>(&envelope, "compatibility-instance", "configuration")
            .unwrap();
        assert_eq!(plain, fixture["plaintext"]);
        let nonce = B64.decode(&envelope.nonce).unwrap();
        // Keep the fixture's original byte order; the JSON map parser may reorder its keys.
        let decrypted = keys
            .cipher(1)
            .unwrap()
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &B64.decode(&envelope.ciphertext).unwrap(),
                    aad: &envelope.aad(),
                },
            )
            .unwrap();
        let encrypted = keys
            .cipher(1)
            .unwrap()
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &decrypted,
                    aad: &envelope.aad(),
                },
            )
            .unwrap();
        assert_eq!(B64.encode(encrypted), envelope.ciphertext);
    }
    #[test]
    fn recovery_authentication_rotation_and_rollback() {
        let keys = VaultKeys::new().unwrap();
        let sender = Uuid::new_v4();
        let payload = json!({"password":"private", "connections":[]});
        let envelope = keys
            .seal("instance", sender, "configuration", 1, &payload)
            .unwrap();
        let recovered = VaultKeys::recover(&keys.recovery_code()).unwrap();
        assert_eq!(
            recovered
                .open::<serde_json::Value>(&envelope, "instance", "configuration")
                .unwrap(),
            payload
        );
        assert!(
            !serde_json::to_string(&envelope)
                .unwrap()
                .contains("private")
        );
        assert!(
            keys.open::<serde_json::Value>(&envelope, "other", "configuration")
                .is_err()
        );
        for field in [
            "sender",
            "object",
            "revision",
            "epoch",
            "ciphertext",
            "nonce",
        ] {
            let mut changed = envelope.clone();
            match field {
                "sender" => changed.sender_id = Uuid::new_v4(),
                "object" => changed.object_id = "other".into(),
                "revision" => changed.revision += 1,
                "epoch" => changed.epoch += 1,
                "ciphertext" => changed.ciphertext.replace_range(
                    ..1,
                    if changed.ciphertext.starts_with('a') {
                        "b"
                    } else {
                        "a"
                    },
                ),
                _ => changed.nonce = B64.encode([9; 12]),
            }
            assert!(
                keys.open::<serde_json::Value>(&changed, "instance", "configuration")
                    .is_err(),
                "{field}"
            );
        }
        let mut watermark = Watermark::default();
        watermark.observe(&envelope).unwrap();
        let mut rotated = keys.clone();
        rotated.rotate().unwrap();
        let new = rotated
            .seal("instance", sender, "configuration", 2, &payload)
            .unwrap();
        assert!(
            keys.open::<serde_json::Value>(&new, "instance", "configuration")
                .is_err()
        );
        assert!(
            rotated
                .open::<serde_json::Value>(&envelope, "instance", "configuration")
                .is_ok()
        );
        watermark.observe(&new).unwrap();
        assert!(watermark.observe(&envelope).is_err());
        assert!(VaultKeys::recover("dc1_weak").is_err());
        assert_ne!(
            keys.seal("instance", sender, "configuration", 1, &payload)
                .unwrap()
                .nonce,
            envelope.nonce
        );
    }
}
