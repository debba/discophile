# Run your own Discophile Connect instance

Discophile Connect is the optional account for Discophile services. The official instance is
`https://connect.discophile.app`, but Discophile lets people choose any instance that implements the
[Discophile Connect protocol](protocol/README.md). This guide explains what such an instance must do and how
to check it.

Discophile works fully without Connect: standalone use and direct connections to your own music servers never
need an account. Run an instance when you want accounts and plans under your own control, for example for a
family, a community, or development.

## What you build

The official service's source code is not public. A compatible instance is your own server, written in any
language or framework, that follows [version 1 of the protocol](protocol/v1.md). It needs nothing from the
official service: no key, license check, or registration.

Version 1 covers four things:

| Part | Endpoint | What it does |
| --- | --- | --- |
| Discovery | `GET /api/v1/server-info` | Describes the instance, its sign-in endpoints, and its capabilities. Public. |
| Sign-in | `/oauth/authorize`, `POST /oauth/token` | OAuth 2.0 authorization code with PKCE (`S256`) for the Discophile apps, with rotating refresh tokens. |
| Revocation | `POST /oauth/revoke` | Ends a delegation immediately, on every server node. |
| Account | `GET /api/v1/account` | The account, its plan, and the plan's entitlements. |

Device pairing, encrypted synchronization, backups, remote control, and playback handoff are not part of
version 1. An instance that offers only accounts and plans is fully compliant.

## Requirements

- **An HTTPS origin**, such as `https://connect.example.org`, with a valid certificate. The origin identifies
  the instance: every endpoint and resource in discovery must use it. Plain `http` is accepted only on
  `localhost`, `127.0.0.1`, and `[::1]` for development.
- **A stable instance identifier** (`instance.id`), for example a UUID. Keep it as long as the instance keeps
  its accounts: apps treat a new identifier at the same address as a different instance and sign people out.
- **A way to identify people.** Email links, passwords, passkeys, or social providers are all fine: the
  protocol does not see how people sign in.
- **A consent page** at the authorization endpoint that shows which app is asking, which instance the person is
  signing in to, and what the app will read, with Allow and Deny.
- **Durable storage** for accounts, plans, authorization codes, and tokens. Store codes and tokens only as
  one-way hashes, and check on every request that the delegation is still valid.

## Steps

1. **Publish discovery.** Serve `GET /api/v1/server-info` with `protocol: "discophile-connect"`,
   `protocolVersions: [1]`, your origin as `instance.origin` and `authorization.issuer`, your endpoints,
   `scopesSupported` including `account:read` and `offline_access`, `resources.api` equal to
   `<origin>/api/v1`, and `capabilities: { "account": { "version": 1 } }`. Publishing the same issuer and
   endpoints at `/.well-known/oauth-authorization-server` is recommended.
2. **Accept the Discophile apps.** They are public clients without a secret. Accept exactly these redirect
   URIs:

   | Client identifier | App | Redirect URI |
   | --- | --- | --- |
   | `discophile-desktop` | Discophile for computers | `http://127.0.0.1:{port}/connect/callback`, any port from 1024 to 65535 |
   | `discophile-ios` | Discophile for iPhone | `it.debba.discophile:/connect/callback` |

   Show an error page, without redirecting, for unknown clients or redirect URIs.
3. **Implement the authorization request.** Require `code_challenge_method=S256`, reject unknown scopes with
   `invalid_scope` and other resources with `invalid_target`, and return `code`, `state`, and preferably `iss`
   to the redirect URI.
4. **Issue and rotate tokens.** Codes are single use and expire within 60 seconds; a reused code revokes
   everything issued from it. Each refresh returns a new refresh token, and presenting a replaced refresh
   token revokes the whole delegation. Send `Cache-Control: no-store` with tokens and account data.
5. **Serve the account.** Return the stable account identifier, name, email address, the effective plan
   (`free`, `connect`, `supporter`, or your own tiers), its expiry, and its entitlements. Mark an entitlement
   `available: true` only when you advertise the capability it needs; in version 1 that means planned benefits
   stay `false`.
6. **Run the conformance checks.** With Node.js 20 or later:

   ```sh
   node protocol/conformance/check.mjs https://connect.example.org
   ```

   They verify discovery, origin binding, OAuth metadata, PKCE and redirect URI enforcement, and error
   formats, without an account and without changing data. Sign-in, refresh, and revocation need a person:
   test them with the apps or follow the [examples](protocol/examples.md).
7. **Connect Discophile.** In a version of Discophile for computers that includes Discophile Connect, open **Settings → Discophile Connect**, enter your
   instance's address under **Custom address**, choose **Use this address**, and sign in. The app accepts the
   instance only if discovery describes the same address and all endpoints belong to it.

## Reference

- [Protocol overview](protocol/README.md)
- [Version 1 specification](protocol/v1.md): every requirement, error, and lifetime in detail.
- [Examples](protocol/examples.md): complete requests and responses.
- [Conformance script](protocol/conformance/check.mjs)

The protocol is a draft. Changes that older apps can ignore keep version 1; incompatible changes will use a new
version, and instances may serve several side by side.

## License

The documents in this folder are licensed under CC BY 4.0, and the conformance script under the MIT License.
See [LICENSE.md](LICENSE.md).
