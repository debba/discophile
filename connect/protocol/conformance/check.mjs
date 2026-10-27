#!/usr/bin/env node
/**
 * Discophile Connect protocol version 1: public conformance checks.
 *
 * Usage: node check.mjs https://connect.example.org
 *
 * Needs Node.js 20 or later and no dependencies. It reads public endpoints, sends requests that a
 * compliant server must reject, and never signs in or modifies data.
 */

const input = process.argv[2];
if (!input) {
  console.error("Usage: node check.mjs <instance URL>");
  process.exit(2);
}

let origin;
try {
  const url = new URL(input);
  if (url.pathname !== "/" || url.search || url.hash || url.username) throw new Error();
  origin = url.origin;
} catch {
  console.error(`Not an instance URL: ${input}`);
  process.exit(2);
}

const results = [];
const record = (level, name, detail = "") => results.push({ level, name, detail });
const pass = (name) => record("pass", name);
const fail = (name, detail) => record("fail", name, detail);
const warn = (name, detail) => record("warn", name, detail);
function check(name, condition, detail) {
  condition ? pass(name) : fail(name, detail);
  return condition;
}

async function request(path, init = {}) {
  const response = await fetch(path.startsWith("http") ? path : origin + path, { redirect: "manual", ...init, headers: { Accept: "application/json", ...(init.headers ?? {}) } });
  let body = null;
  const text = await response.text();
  try {
    body = text ? JSON.parse(text) : null;
  } catch {
    body = text;
  }
  return { response, body };
}

const endpointsOf = (document) => ["authorizationEndpoint", "tokenEndpoint", "revocationEndpoint"].map((name) => document.authorization?.[name]).filter((v) => typeof v === "string");

const sameOrigin = (value) => {
  try {
    return new URL(value).origin === origin;
  } catch {
    return false;
  }
};

// Discovery
const { response: infoResponse, body: info } = await request("/api/v1/server-info");
if (!check("server-info answers 200 with JSON", infoResponse.status === 200 && info && typeof info === "object", `status ${infoResponse.status}`)) {
  report();
}
check("protocol is discophile-connect", info.protocol === "discophile-connect", JSON.stringify(info.protocol));
check("protocolVersions includes 1", Array.isArray(info.protocolVersions) && info.protocolVersions.includes(1), JSON.stringify(info.protocolVersions));
check("instance.id is a non-empty string of at most 128 characters", typeof info.instance?.id === "string" && info.instance.id.length > 0 && info.instance.id.length <= 128);
check("instance.origin equals the origin used", info.instance?.origin === origin, `${info.instance?.origin} != ${origin}`);
check("authorization.issuer equals the origin", info.authorization?.issuer === origin, info.authorization?.issuer);
for (const name of ["authorizationEndpoint", "tokenEndpoint", "revocationEndpoint"]) {
  check(`authorization.${name} has the instance origin`, sameOrigin(info.authorization?.[name]), info.authorization?.[name]);
}
check("scopesSupported includes account:read", info.authorization?.scopesSupported?.includes("account:read"));
check("scopesSupported includes offline_access", info.authorization?.scopesSupported?.includes("offline_access"));
for (const client of ["discophile-desktop", "discophile-android", "discophile-ios"]) {
  check(`nativeClients includes ${client}`, info.authorization?.nativeClients?.includes(client));
}
check("resources.api is <origin>/api/v1", info.resources?.api === `${origin}/api/v1`, info.resources?.api);
for (const [name, value] of Object.entries(info.resources ?? {})) {
  if (!sameOrigin(value)) fail(`resources.${name} has the instance origin`, value);
}
for (const value of [endpointsOf(info), Object.values(info.resources ?? {})].flat()) {
  try {
    const url = new URL(value);
    if (url.search || url.hash) fail("endpoints and resources have no query or fragment", value);
  } catch {}
}
check("capabilities is an object", info.capabilities && typeof info.capabilities === "object" && !Array.isArray(info.capabilities));
check("account capability version 1", info.capabilities?.account?.version === 1, JSON.stringify(info.capabilities?.account));
if (info.links?.dashboard !== undefined) check("links.dashboard has the instance origin", sameOrigin(info.links.dashboard), info.links.dashboard);

const endpoints = info.authorization ?? {};
const api = info.resources?.api ?? `${origin}/api/v1`;

// OAuth metadata (recommended)
const { response: metaResponse, body: meta } = await request("/.well-known/oauth-authorization-server");
if (metaResponse.status === 200 && meta && typeof meta === "object") {
  check("OAuth metadata issuer matches", meta.issuer === origin, meta.issuer);
  check("OAuth metadata supports S256", meta.code_challenge_methods_supported?.includes("S256"));
  check("OAuth metadata token endpoint matches", meta.token_endpoint === endpoints.tokenEndpoint, meta.token_endpoint);
} else {
  warn("OAuth authorization server metadata", `not published (status ${metaResponse.status}); recommended by the protocol`);
}

// Account API errors
{
  const { response, body } = await request(`${api}/account`);
  check("account without a token answers 401", response.status === 401, `status ${response.status}`);
  check("401 carries error invalid_token", body?.error === "invalid_token", JSON.stringify(body));
  check("401 carries a Bearer challenge", /^Bearer\b/i.test(response.headers.get("www-authenticate") ?? ""), response.headers.get("www-authenticate"));
}
{
  const { response } = await request(`${api}/account`, { headers: { Authorization: "Bearer not-a-real-token" } });
  check("account with an unknown token answers 401", response.status === 401, `status ${response.status}`);
}
if (!info.capabilities?.devices) {
  const { response, body } = await request(`${api}/devices`);
  check("endpoints of capabilities not offered answer 404 not_found", response.status === 404 && body?.error === "not_found", `status ${response.status}`);
}

// Token and revocation endpoints
const form = (fields) => ({ method: "POST", headers: { "Content-Type": "application/x-www-form-urlencoded" }, body: new URLSearchParams(fields) });
{
  const { response, body } = await request(endpoints.tokenEndpoint, form({ grant_type: "authorization_code", code: "invalid", redirect_uri: "http://127.0.0.1:50000/connect/callback", client_id: "discophile-desktop", code_verifier: "a".repeat(43) }));
  check("an invalid code is rejected with 400 invalid_grant", response.status === 400 && body?.error === "invalid_grant", `${response.status} ${JSON.stringify(body)}`);
  check("token errors are not cached", (response.headers.get("cache-control") ?? "").includes("no-store"), response.headers.get("cache-control"));
}
{
  const { response, body } = await request(endpoints.tokenEndpoint, form({ grant_type: "password", client_id: "discophile-desktop", username: "a", password: "b" }));
  check("unsupported grants are rejected", response.status === 400 && body?.error === "unsupported_grant_type", `${response.status} ${JSON.stringify(body)}`);
}
{
  const { response, body } = await request(endpoints.tokenEndpoint, form({ grant_type: "refresh_token", refresh_token: "invalid", client_id: "discophile-desktop" }));
  check("an invalid refresh token is rejected with invalid_grant", response.status === 400 && body?.error === "invalid_grant", `${response.status} ${JSON.stringify(body)}`);
}
{
  const { response } = await request(endpoints.revocationEndpoint, form({ token: "unknown", client_id: "discophile-desktop" }));
  check("revoking an unknown token answers 200", response.status === 200, `status ${response.status}`);
}

// Authorization endpoint: never redirect to an untrusted address
const challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
const authorize = (fields) => {
  const url = new URL(endpoints.authorizationEndpoint);
  for (const [k, v] of Object.entries({ response_type: "code", scope: "account:read offline_access", state: "conformance", code_challenge: challenge, code_challenge_method: "S256", resource: api, ...fields })) {
    if (v !== undefined) url.searchParams.set(k, v);
  }
  return request(url.toString(), { headers: { Accept: "text/html" } });
};
const redirectsTo = (response, prefix) => response.status >= 300 && response.status < 400 && (response.headers.get("location") ?? "").startsWith(prefix);
{
  const { response } = await authorize({ client_id: "not-a-client", redirect_uri: "https://attacker.example/cb" });
  check("an unknown client is not redirected", !redirectsTo(response, "https://attacker.example"), response.headers.get("location"));
}
{
  const { response } = await authorize({ client_id: "discophile-desktop", redirect_uri: "https://attacker.example/cb" });
  check("an unregistered redirect URI is not used", !redirectsTo(response, "https://attacker.example"), response.headers.get("location"));
}
{
  const { response } = await authorize({ client_id: "discophile-desktop", redirect_uri: "http://localhost:50000/connect/callback" });
  check("discophile-desktop rejects localhost redirects", !redirectsTo(response, "http://localhost"), response.headers.get("location"));
}
{
  const { response } = await authorize({ client_id: "discophile-desktop", redirect_uri: "http://127.0.0.1:50000/connect/callback", code_challenge_method: "plain", code_challenge: "x".repeat(43) });
  const location = response.headers.get("location") ?? "";
  if (redirectsTo(response, "http://127.0.0.1:50000/connect/callback")) {
    const params = new URL(location).searchParams;
    check("plain PKCE is rejected with invalid_request", params.get("error") === "invalid_request" && params.get("state") === "conformance", location);
    if (params.has("iss")) check("error redirects carry the issuer", params.get("iss") === origin, params.get("iss"));
  } else {
    warn("plain PKCE", `the server answered ${response.status} without redirecting; it may ask the person to sign in first`);
  }
}

report();

function report() {
  const symbols = { pass: "PASS", fail: "FAIL", warn: "WARN" };
  for (const r of results) console.log(`${symbols[r.level]}  ${r.name}${r.detail && r.level !== "pass" ? ` (${r.detail})` : ""}`);
  const failed = results.filter((r) => r.level === "fail").length;
  console.log(`\n${results.length - failed} of ${results.length} checks passed${failed ? `, ${failed} failed` : ""}.`);
  process.exit(failed ? 1 : 0);
}
