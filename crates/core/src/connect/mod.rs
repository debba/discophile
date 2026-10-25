//! Discophile Connect: optional accounts on a Connect instance, following the Connect protocol version 1
//! (instance discovery, native sign-in, the account and its plan).
//!
//! - [`instance`]: the instance's canonical origin (`https://connect.discophile.app` by default).
//! - [`discovery`]: `GET /api/v1/server-info`, bound to the origin the client used.
//! - [`authorization`]: the browser sign-in with PKCE and a loopback redirect.
//! - [`credential`]: tokens and the account namespace (origin and account identifier).
//! - [`account`]: the account, its plan and entitlements.
//! - [`client`]: the HTTP requests, with [`ConnectError`].
//!
//! The optional native services extension encrypts selected configuration and playback messages. A
//! plan never restricts local playback or direct connections to the person's own servers. Changing instance
//! requires a new sign-in; tokens and identifiers are never sent to another origin.
//!
//! [`handoff`] defines playback messages; [`vault`] authenticates their encrypted transport.

pub mod account;
pub mod authorization;
pub mod client;
pub mod credential;
pub mod discovery;
mod error;
pub mod handoff;
pub mod instance;
pub mod vault;
pub mod services;

pub use account::{Account, AccountInfo, Entitlement, EntitlementKind, EntitlementStatus, Plan, PlanSource, Tier};
pub use authorization::AuthorizationRequest;
pub use client::ConnectClient;
pub use credential::{AccountNamespace, ConnectCredential, TokenSet};
pub use discovery::ServerInfo;
pub use error::ConnectError;
pub use handoff::{
    ConnectMessage, DeviceInfo, DeviceKind, MDNS_SERVICE, PROTOCOL_VERSION, PlaybackContent, PlaybackSnapshot, RemoteCommand,
};
pub use instance::{DEFAULT_INSTANCE, InstanceOrigin, InstanceUrlError};
