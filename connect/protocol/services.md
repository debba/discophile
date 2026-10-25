# Connect services, version 1

The account protocol stays at version 1. Capabilities `devices`, `sync`, `backup`,
`remote`, and `handoff` each advertise version 1. Desktop clients explicitly request
the additional `connect:manage` OAuth scope. Old account-only clients keep working.
The iPhone client is unchanged and must not advertise support for these services.

## Cryptographic specification and review boundary

Suite: AES-256-GCM, 256-bit uniformly random keys, 96-bit random nonces, 128-bit tags.
Use RustCrypto `aes-gcm` in Rust and CryptoKit AES.GCM for the compatibility vectors.
Keys and nonces use the operating system RNG. Ciphertext includes the appended tag.
Both nonce and ciphertext are unpadded base64url. No password-derived keys are used.

An envelope has `version:1`, `instanceId`, `vaultId` (UUID), `epoch` (positive integer),
`senderId` (device UUID), `objectId`, `revision`, `nonce`, and `ciphertext`.
Authenticated data is UTF-8 JSON of this ordered array, without whitespace:
`["discophile-connect-vault",1,instanceId,vaultId,epoch,senderId,objectId,revision]`.
Identifiers are ASCII; UUIDs must use canonical lowercase spelling. Configurations use objectId `configuration`; presence uses
`state:<deviceId>`; messages use `message:<targetId>:<messageId>:<kind>`.
Every encryption obtains a fresh random nonce, including retries of a changed object.

The native client stores a recovery bundle separately from its OAuth credential,
namespaced by instance ID and account ID in the secrets store. The bundle contains
the vault UUID, current epoch and historical epoch keys. A recovery code is `dc1_`
followed by base64url of that JSON. It is a secret, not a short pairing PIN. It is
shown only on explicit request, never logged, sent to Connect, or included in email.
This release uses **personal recovery**, not public-key pairing: on a second trusted
computer the owner enters the recovery code, the app authenticates the existing
configuration, then registers a new device. Login alone cannot open the vault.
First creation never overwrites a pre-existing vault. Loss of all keys is unrecoverable.

The secrets store uses the OS keychain in release builds, with the app's existing
owner-protected file fallback where unavailable; debug builds use the protected
development file. This is device security, not a claim against malware on that device.
Browser decryption is excluded. Server-side account, device names, timestamps,
declared connection count, payload lengths and message routing are not encrypted.

The client authenticates envelope context before decryption. Known configuration
revision and epoch watermarks are persisted. A lower revision/epoch or a different
envelope at an already observed revision is rejected. Writes use expectedRevision
and fail on conflicts; clients offer an explicit choice, never silently replace
concurrent changes. Connection removals are represented by absence in the next
complete shared configuration; importing never deletes local connections/downloads.
The shared selection is explicit and separate from all local connections. The free
client selects at most one server. The service enforces the declared record count;
it cannot inspect or prove the contents of an encrypted record.

Revoking a device blocks it immediately and marks the vault as requiring rotation.
New configuration, presence and command writes are blocked until rotation. A trusted
computer generates a fresh epoch key and commits a new encrypted configuration with
CAS. Other computers must import the new recovery bundle before continuing. The
bundle retains historical keys for backups. Revocation cannot erase previously
copied content. A revoked device cannot reuse its old registration ID. Cloud deletion
is separately available regardless of tier. No administrative tool can decrypt data.

An actively malicious service can deny service, withhold updates, and replay state
not yet observed by a fresh installation. Local watermarks protect already observed
state, not global freshness. Shared epoch keys authenticate membership, not individual
human identity; every paired device is trusted to read and change shared settings.
Do not describe this design as independently audited. Negative tests and a CryptoKit
interoperability fixture are required before enabling the service in production.

## HTTP API

`GET /api/v1/plans` returns the commercial catalog (EUR cents, monthly/yearly reference
prices, email activation). No payment, renewal or automatic tier change occurs.

`POST /api/v1/connect` accepts JSON and bearer authorization for the API audience,
`connect:manage` scope, no cookie authentication. Maximum body 512 KiB; ciphertext
maximum 350 KiB. Success is JSON. Errors use `error` and `message`, no private data.
All state-changing operations lock the account row and check its current effective
tier; device registration count cannot race. Device IDs are globally unique UUIDs.

Operations (`action`):

- `inspect`: vault metadata/configuration, no key or plaintext. Used for recovery.
- `join`: `deviceId`, `deviceSecret` (43-char random base64url), `name`, `vaultId`,
  `epoch`, optional initial `envelope` and `connectionCount`. First vault uses epoch 1,
  revision 1. Rebinding an existing active registration requires the same device secret.
- All remaining operations require `deviceId` bound to the current OAuth delegation.
- `poll`: current vault, active device names/presence, up to 3 pending messages,
  backup metadata, effective plan. Presence becomes offline after 30 seconds.
- `write`: `expectedRevision`, `connectionCount`, `envelope`; CAS creates revision + 1.
- `rotate`: as write, but epoch + 1; allowed only after revocation.
- `state`: encrypted playback state at a monotonically increasing device revision.
- `send`: `id`, `targetId`, `kind` (`remote`/`handoff`), `envelope`; message ID is
  idempotent for the exact same content and otherwise conflicts. TTL is 60 seconds,
  target must have polled in the last 30 seconds. Both sender and receiver must be
  active, within the effective device allowance, and have the current paid entitlement.
- `ack`: array of at most 50 message IDs, deletes only this device's messages.
- `backup`: saves the current encrypted configuration, deduplicated by revision;
  at most 64 snapshots for 30 days. Clients request one automatic daily snapshot
  and after configuration changes, plus explicit manual snapshots.
- `restore`: `id` reads a retained encrypted backup. Reading/export/deletion remain
  available on Free; writing a restored configuration obeys the current sync limit.
- `revoke`: `targetId`; removes that registration, its state and queued messages,
  increments no epoch until the next rotate, marks rotation required. Can revoke self.
- `delete`: deletes this account's cloud vault, registrations, backups and messages.

Excess devices after downgrade are suspended deterministically by creation time then
ID (first 2 remain usable). They can always revoke/delete/export. No local music,
downloads or configurations are removed. Backup retention is enforced on reads and
writes; records are physically pruned on the next service operation.

## Playback and delivery

Native receivers poll every 3 seconds while running. No audio passes through Connect.
Sleeping/closed clients are unavailable. State is encrypted and scoped to its device.
Commands contain a unique message ID plus timestamp, are authenticated, expire after
60 seconds, and are remembered locally before application to avoid replay after restart. Native clients retain 4096 received IDs, covering the maximum 3600 operations accepted within a 60-second message lifetime across two fixed account rate-limit windows (1800 operations/minute); per-delegation limit is 120/minute.
Volume/seek ranges and queue length are validated locally. Handoff uses the existing
PlaybackSnapshot music/radio format. The destination verifies the matching configured
library, prepares playback, and sends TransferComplete only after audio output reports
playing (or ready for an intentionally paused snapshot). Until that acknowledgement,
the source keeps playing. Rejection/timeout leaves source playback untouched. A late
acknowledgement is ignored if the source track or user playback state changed.
