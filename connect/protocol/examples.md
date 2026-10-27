# Protocol version 1 examples

Complete requests and responses for a sign-in from Discophile for computers against `https://connect.example.org`. Values are shortened or illustrative; tokens are opaque and differ on every instance.

## 1. Discovery

```http
GET /api/v1/server-info HTTP/1.1
Host: connect.example.org
Accept: application/json
User-Agent: Discophile/0.2.0
```

```http
HTTP/1.1 200 OK
Content-Type: application/json; charset=utf-8
Cache-Control: public, max-age=300

{
  "protocol": "discophile-connect",
  "protocolVersions": [1],
  "instance": { "id": "1f6e0c9a-7c8d-4a51-a2b3-5d6e7f809a1b", "origin": "https://connect.example.org", "name": "Example Connect" },
  "authorization": {
    "issuer": "https://connect.example.org",
    "authorizationEndpoint": "https://connect.example.org/oauth/authorize",
    "tokenEndpoint": "https://connect.example.org/oauth/token",
    "revocationEndpoint": "https://connect.example.org/oauth/revoke",
    "scopesSupported": ["account:read", "offline_access"],
    "nativeClients": ["discophile-desktop", "discophile-android", "discophile-ios"]
  },
  "resources": { "api": "https://connect.example.org/api/v1" },
  "capabilities": { "account": { "version": 1 } },
  "links": { "dashboard": "https://connect.example.org/account" }
}
```

The client checks that `instance.origin`, `authorization.issuer`, every endpoint, and `resources.api` all use `https://connect.example.org`.

## 2. Authorization

The client listens on `127.0.0.1:53682`, generates a verifier and state, and opens the system browser:

```text
code_verifier  = dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk
code_challenge = E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM
state          = Ys0X3s2b6n6q4TQm7bH2b8m3oUq1i8T0yS0pWm2bJXc
```

```text
https://connect.example.org/oauth/authorize
  ?response_type=code
  &client_id=discophile-desktop
  &redirect_uri=http%3A%2F%2F127.0.0.1%3A53682%2Fconnect%2Fcallback
  &scope=account%3Aread%20offline_access
  &state=Ys0X3s2b6n6q4TQm7bH2b8m3oUq1i8T0yS0pWm2bJXc
  &code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM
  &code_challenge_method=S256
  &resource=https%3A%2F%2Fconnect.example.org%2Fapi%2Fv1
```

After the person signs in and allows access, the browser requests:

```http
GET /connect/callback?code=dcc_Q2p9...&state=Ys0X3s2b6n6q4TQm7bH2b8m3oUq1i8T0yS0pWm2bJXc&iss=https%3A%2F%2Fconnect.example.org HTTP/1.1
Host: 127.0.0.1:53682
```

If the person denies access:

```http
GET /connect/callback?error=access_denied&error_description=The+person+denied+access.&state=Ys0X3s2b6n6q4TQm7bH2b8m3oUq1i8T0yS0pWm2bJXc&iss=https%3A%2F%2Fconnect.example.org HTTP/1.1
Host: 127.0.0.1:53682
```

## 3. Token

```http
POST /oauth/token HTTP/1.1
Host: connect.example.org
Content-Type: application/x-www-form-urlencoded

grant_type=authorization_code&code=dcc_Q2p9...&redirect_uri=http%3A%2F%2F127.0.0.1%3A53682%2Fconnect%2Fcallback&client_id=discophile-desktop&code_verifier=dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk&resource=https%3A%2F%2Fconnect.example.org%2Fapi%2Fv1
```

```http
HTTP/1.1 200 OK
Content-Type: application/json; charset=utf-8
Cache-Control: no-store

{
  "access_token": "dca_3f6Yb1yQyT0Qv0u1oJqvH2wqf6wQ8tXg...",
  "token_type": "Bearer",
  "expires_in": 3600,
  "refresh_token": "dcr_xQ2h8mW2z7m0rS4yU6cQ1aB9kP3nE5tL...",
  "scope": "account:read offline_access"
}
```

A second exchange of the same code:

```http
HTTP/1.1 400 Bad Request
Content-Type: application/json; charset=utf-8
Cache-Control: no-store

{ "error": "invalid_grant", "error_description": "The authorization code was already used." }
```

The tokens issued from that code stop working too.

## 4. Account

```http
GET /api/v1/account HTTP/1.1
Host: connect.example.org
Accept: application/json
Authorization: Bearer dca_3f6Yb1yQyT0Qv0u1oJqvH2wqf6wQ8tXg...
```

```http
HTTP/1.1 200 OK
Content-Type: application/json; charset=utf-8
Cache-Control: no-store

{
  "instanceId": "1f6e0c9a-7c8d-4a51-a2b3-5d6e7f809a1b",
  "account": { "id": "u_6sQb2kL8wN4tR1", "name": "Ada Example", "email": "ada@example.com", "emailVerified": true },
  "plan": {
    "tier": "free",
    "name": "Free",
    "source": "default",
    "expiresAt": null,
    "entitlements": []
  }
}
```

Without a valid token:

```http
HTTP/1.1 401 Unauthorized
WWW-Authenticate: Bearer resource_metadata="https://connect.example.org/.well-known/oauth-protected-resource/api/v1", error="invalid_token"
Content-Type: application/json; charset=utf-8

{ "error": "invalid_token", "message": "The access token is missing, expired, or revoked." }
```

## 5. Refresh

```http
POST /oauth/token HTTP/1.1
Host: connect.example.org
Content-Type: application/x-www-form-urlencoded

grant_type=refresh_token&refresh_token=dcr_xQ2h8mW2z7m0rS4yU6cQ1aB9kP3nE5tL...&client_id=discophile-desktop
```

The response has the same shape as the first token response, with a new refresh token. Presenting the old refresh token again revokes the delegation:

```http
HTTP/1.1 400 Bad Request

{ "error": "invalid_grant", "error_description": "The refresh token is invalid, expired, or revoked." }
```

## 6. Sign-out

```http
POST /oauth/revoke HTTP/1.1
Host: connect.example.org
Content-Type: application/x-www-form-urlencoded

token=dcr_Lm4...&token_type_hint=refresh_token&client_id=discophile-desktop
```

```http
HTTP/1.1 200 OK
Cache-Control: no-store
```

Every token of the delegation stops working immediately. The client deletes its stored tokens whether or not this request succeeds.

## 7. Discophile for iPhone

The iPhone app uses `client_id=discophile-ios` and `redirect_uri=it.debba.discophile:/connect/callback`, through `ASWebAuthenticationSession`. The callback arrives as:

```text
it.debba.discophile:/connect/callback?code=dcc_...&state=...&iss=https%3A%2F%2Fconnect.example.org
```

Every other step is identical.
