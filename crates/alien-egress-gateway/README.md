# Sandbox egress gateway

The gateway verifies an Ed25519 sandbox identity and reads that sandbox's current
policy from the deployment KV. It resolves destinations itself and connects only
to checked IPv4 addresses. Private, link-local, metadata, multicast and configured
deployment/overlay ranges are always denied. IPv6 is not an outbound candidate.

Run `alien-egress-gateway` with `ALIEN_EGRESS_GATEWAY_CONFIG` containing:

```json
{
  "listen": "0.0.0.0:3128",
  "admin": "0.0.0.0:3129",
  "generationStore": {"binding": "egress-state"},
  "audiences": ["deployment/box"],
  "defaultPolicies": {},
  "keys": [{"kid": "current", "publicKey": "BASE64_RAW_ED25519_PUBLIC_KEY"}],
  "deniedRanges": ["10.0.0.0/8", "10.244.0.0/16"],
  "credentials": []
}
```

The image runs as UID/GID 65532, needs no capabilities, and supports a read-only
root filesystem. Provide the existing KV binding environment. Local KV requires
a writable data volume shared by replicas; cloud KV requires no writable files.
A config-file path is also accepted as the first argument. `admin` defaults to
3129. Features select providers: `aws`, `cloud` (AWS, GCP, Azure and Kubernetes
base-cloud stores), and `local`. All replicas must use the same KV. SQLite Local
replicas must share local disk, not a multi-host network filesystem. Redis is not
implemented by Alien bindings.

Tokens contain only `{iss,aud,sub,iat,exp}`, with `iss: "alien-sandbox"`, a maximum
24-hour lifetime and a 4 KiB encoded size limit. Neither a policy nor a generation
appears in a proxy token. Missing policy records deny access, even if the config
contains a create default. The binding applies create defaults explicitly.

Every new connection reads the policy reference from KV; only verified content
is cached, in at most 128 entries. Updates and deletion therefore need no token
replacement and do not wait for a cache TTL. Store failures deny access. Up to
16 fixed 16 KiB chunk slots per sandbox support the 256 KiB / 4096-target limit
on every KV provider. A digest detects torn reads while the binding serializes
writes per sandbox. Interrupted writes may deny connections until retried.
Replacement reuses slots; deletion removes the record first, then every slot.
This keeps storage bounded without a second state service or retained versions.

The proxy accepts `Proxy-Authorization: Bearer <JWS>` and Basic credentials from
`http://alien:<JWS>@gateway:3128`. DNS-name grants require TLS with matching SNI
or HTTP with matching Host, on any port. A rule without a port matches every
port; `*.example.com` includes `example.com` and all its subdomains. Other
protocols, including server-first protocols, require explicit IP/CIDR grants.
Internet policy does not grant raw IP literals. Spliced TLS cannot inspect the
encrypted HTTP Host: domain fronting remains an accepted limit. Intercepted TLS
checks HTTP Host. HTTP handles one request per connection and advertises close.
Headers, ClientHello, connection concurrency and connection lifetime are bounded.

Admin port 3129 is plain HTTP restricted to management workloads. Requests carry
an Ed25519 JWS in `Authorization: Bearer`, with header
`typ: "alien-egress-admin"` and claims `{iss,aud,sub,iat,exp,method,policy?}`.
The lifetime is at most 60 seconds. `method` binds the HTTP method; for PUT,
`policy` is `sha256:<hex>` of the normalized, sorted, compact policy JSON.
It is an admin-body authenticator, never a proxy-token claim.

- `PUT /v1/policies {aud,sub,policy}` replaces the policy and returns 204.
- `DELETE /v1/policies {aud,sub}` removes authorization and content; returns 204
  even when already absent. Its grant must not contain `policy`.
- `GET /health` is unauthenticated. `GET /metrics` requires an admin grant.

Identity, method and policy must match the signed request. The binding serializes
updates per sandbox; writes use last-write-wins semantics. A captured admin
request remains replayable during its 60-second validity, so the admin network
must be inaccessible to guests. Logs contain one JSON record per connection,
without tokens, request headers or secret values.

Optional credential rules contain `aud`, exact `host`, `port`, uppercase `method`,
`path`, `header`, `vaultBinding`, `secret`, and optional `valuePrefix`. Paths match
exactly unless they end in `/`, which selects that subtree. Ambiguous encoded paths
are refused during interception. Header values come from the deployment vault and
are never logged. The gateway replaces the matching header, verifies the upstream
TLS certificate, and does not follow redirects. An upstream service can deliberately
reflect a credential; only configure trusted origins. `upstreamCaCertificates` may
add deployment-owned trust roots encoded as base64 DER.

`POST /v1/ca {aud,sub}` with a matching admin JWS creates or returns a sandbox's
public CA certificate as `{certificate: "PEM"}`. The private CA remains in the
shared KV. The gateway therefore needs explicit data-read/write permission on that
store, and data-read permission on each configured credential vault. Deliver the
public CA through the agent's `PUT /v1/egress {token,caCert?}`. Trust bundle paths
are passed to standard tools; applications with their own trust stores must import
the public CA explicitly.

For a gateway-backed resource the agent starts without a token. Health reports
`awaiting-egress-policy`, and
session routes are blocked until token installation and the external path check
succeed. On AWS the Run hook receives the AWS envelope
`{microvmId,runHookPayload}`; the payload is a JSON string `{gateway,audience}`.
The injected MicroVM ID is checked against the token subject. Elsewhere the backend
sets `ALIEN_SANDBOX_SESSION_ID`, `ALIEN_SANDBOX_EGRESS_AUDIENCE`, and
`ALIEN_SANDBOX_EGRESS_GATEWAY`. Tokens are never supplied through environment or
image state. Policy updates go only to the gateway; token installation happens at create
and renewal. Updates affect new connections. Existing tunnels remain open until close or the connection deadline.

A resource declared `none` uses a loopback-only network and has no gateway or token.
The agent verifies that direct internet fails before serving. A closed runtime policy still requires the gateway and an identity token.

Proxy-aware commands bypass the proxy for localhost and loopback; declared `none`
does not advertise a proxy. Synthetic DNS answers expire after 60 seconds and
renew on lookup. The agent retains at most 4096 names, reclaims expired entries
when full, and never reuses an
issued synthetic address, so stale answers cannot connect to a different name.
Clients must resolve again after expiry. After 131070 allocations in one agent
lifetime, new names fail closed; create a fresh sandbox to reset that limit.

Validation tools live in `tests/` and `fuzz/`. Build the two release binaries, build
`tests/Dockerfile.agent` with `alien-sandbox-agent` in its Docker build context, then
run `python3 tests/docker_conformance.py --image IMAGE --agent-image AGENT_IMAGE
--output /tmp/egress-proof`. The runner requires Python `cryptography` and a Docker
daemon, uses isolated controlled TLS origins plus the public TCP canary for the
open-network negative control, benchmarks verified TLS transfers,
and removes its containers/networks. Run each parser with `cargo +nightly fuzz run
connect -- -max_total_time=60` and `cargo +nightly fuzz run client_hello --
-max_total_time=60` from this crate directory. Keep output outside the repository.
