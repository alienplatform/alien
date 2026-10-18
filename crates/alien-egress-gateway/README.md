# Sandbox egress gateway

The gateway checks each connection against a deployment-owned bound and an Ed25519
sandbox token. It resolves the destination itself and connects only to checked IPv4
addresses. Private, link-local, metadata, multicast and configured deployment ranges
are always denied. IPv6 is not a candidate for outbound connections.

Run `alien-egress-gateway` with `ALIEN_EGRESS_GATEWAY_CONFIG` containing:

```json
{
  "listen": "0.0.0.0:3128",
  "adminListen": "0.0.0.0:3129",
  "generationBinding": "egress-state",
  "bounds": {"deployment/box": {"allow": ["api.example.com", "example.com:80"]}},
  "keys": [{"kid": "current", "publicKey": "BASE64_RAW_ED25519_PUBLIC_KEY"}],
  "deniedRanges": ["10.0.0.0/8"],
  "credentials": []
}
```

A config-file path is also accepted as the first argument. `adminListen` defaults
to port 3129 and `generationBinding` to `egress-state`. Supply the normal Alien KV
binding environment for that name. Build features select providers: `aws`, `cloud`
(AWS, GCP and Kubernetes base-cloud stores), and `local`.

Tokens contain `{iss,aud,sub,gen,iat,exp,policy}`; `policy` is `sha256:<hex>` of
the normalized, sorted, compact policy JSON. Tokens are limited to 4 KiB. The
admin API installs the full policy before the agent receives its token. A missing
record, a different generation (older or newer), or a different digest denies access.

The existing KV conditional-write API provides the durable generation and policy
reference. Every authorization reads that record; store failures deny access. A
bounded cache holds only verified immutable policy contents. Policies use 16 KiB
content-addressed chunks so every KV backend supports the same size limit. Chunks
are written before the atomic reference update. Records and chunks have no TTL;
deployment teardown removes the store, and superseded policy content consumes
storage until then. This avoids another state service and stale-generation windows
at the cost of a KV read per connection and retained policy versions. All
replicas must use the same binding. Local replicas must share a local disk; the
local SQLite provider does not support multi-host network filesystems. Kubernetes
uses its base-cloud KV; the Redis binding is not implemented by Alien bindings.

The proxy port accepts `Proxy-Authorization: Bearer <JWS>`. Port 443 requires a TLS
ClientHello whose SNI equals the CONNECT destination. TLS on other explicit ports
also checks its initial ClientHello. Port 80, including CONNECT
on port 80, requires matching Host and request authority. Other explicit ports
support opaque TCP protocols such as SSH, including server-first greetings. Upgrades
inside an opaque protocol are not inspected. Spliced TLS does not inspect encrypted
HTTP Host; use credential interception when HTTP authority inspection is required.
Plain and intercepted HTTP handle one request per connection and advertise close.
Interception offers HTTP/1.1 only. Headers and ClientHello have size and time limits;
concurrent connections and total connection lifetime are bounded.

The admin listener is plain HTTP inside the deployment network. Sandbox network
rules must permit only port 3128 and must deny 3129. Admin calls carry an Ed25519
JWS in `Authorization: Bearer`, using the deployment's signing keys and mandatory
header `typ: "alien-egress-admin"`. Claims are
`{iss:"alien-sandbox",aud,sub,gen,iat,exp,policy}`; the lifetime is at most 60 seconds.
The `policy` claim is the policy digest; it may be omitted for CA/metrics grants.
`POST /v1/generations {aud,sub,gen,policy:<full policy>}` must match the signed
identity, generation and digest, and must be a subset of the configured bound.
Successful updates return 204; a superseded update or a different policy for an
already-installed generation returns 409. A concurrent conditional-write conflict
returns 503 for the caller to retry. Captured updates are idempotent and cannot be
retargeted. This
listener returns no private key or injected secret. `/health` is unauthenticated;
`/metrics` requires an admin JWS. Logs contain one JSON record per proxy connection.

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
image state. Updates are serialized, checked through the gateway, and affect new
connections. Existing tunnels remain open until close or the connection deadline.

A resource declared `none` uses a loopback-only network and has no gateway or token.
The agent verifies that direct internet fails before serving. Narrowing a resource
declared `internet` or `allow` to `none` still requires the gateway and a token.

Validation tools live in `tests/` and `fuzz/`. Build the two release binaries, build
`tests/Dockerfile.agent` with `alien-sandbox-agent` in its Docker build context, then
run `python3 tests/docker_conformance.py --image IMAGE --agent-image AGENT_IMAGE
--output /tmp/egress-proof`. The runner requires Python `cryptography` and a Docker
daemon, uses isolated controlled TLS origins plus the public TCP canary for the
open-network negative control, benchmarks verified TLS transfers,
and removes its containers/networks. Run each parser with `cargo +nightly fuzz run
connect -- -max_total_time=60` and `cargo +nightly fuzz run client_hello --
-max_total_time=60` from this crate directory. Keep output outside the repository.
