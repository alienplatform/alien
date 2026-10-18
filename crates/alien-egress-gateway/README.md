# Sandbox egress gateway

The gateway verifies an Ed25519 sandbox identity and reads that sandbox's current
policy from bounded memory replicated across gateway instances. It resolves destinations itself and connects only
to checked IPv4 addresses. Private, link-local, metadata, multicast and configured
deployment/overlay ranges are always denied. IPv6 is not an outbound candidate.

At boot, `alien-egress-gateway` reads `sandbox-egress-gateway-config` from its
`secrets` vault binding. It refreshes every 60 seconds and atomically publishes
a validated config/key snapshot. A failed or timed-out refresh retains the last
successful snapshot for at most ten minutes. After that window, new connections
are denied and admin health returns 503 until refresh succeeds. This bounded
window can delay key or range revocation; policy reads never use stale fallback.
The gateway reads only this entry; it never reads `sandbox-egress-signing-key`.
Its service account needs read access to the config entry, which includes public
verification keys and the gateway-only replica secret. Other workloads must not
read this entry. Listener, replica-secret, discovery and memory-budget changes require a restart.
Keep old public verification keys until all records signed by them have been
replaced or retired; retained records have no expiry.

For local/tests, `ALIEN_EGRESS_GATEWAY_CONFIG` overrides vault loading and
periodic refresh. The config is:

```json
{
  "listen": "0.0.0.0:3128",
  "admin": "0.0.0.0:3129",
  "peers": {"dns": "sandbox-egress-gateway.svc", "port": 3129},
  "replicaSecret": "BASE64_GATEWAY_ONLY_32_BYTE_SECRET",
  "audiences": ["deployment/box"],
  "defaultPolicies": {},
  "keys": [{"kid": "current", "publicKey": "BASE64_RAW_ED25519_PUBLIC_KEY"}],
  "deniedRanges": ["10.0.0.0/8", "10.244.0.0/16"],
  "credentials": []
}
```

The image runs as UID/GID 65532, needs no capabilities, and supports a read-only
root filesystem. No policy KV, writable volume or policy-store permission is
required. Policy storage uses half the finite cgroup v2 `memory.max` limit; a
1 GiB container therefore has a 512 MiB record budget. Set `policyMemoryBytes`
explicitly when no finite cgroup limit is available; otherwise startup fails.
Snapshots stream signed records in bounded pages without duplicating the store.
A config-file path is also accepted as a static override in the first
argument. `admin` defaults to 3129. Features select vault providers: `aws`, `cloud`
(AWS, GCP, Azure and Kubernetes), and `local`. Omit `peers` for a single local
replica. See [the replication protocol](REPLICATION.md) for static peers and wire
formats.

Tokens contain only `{iss,aud,sub,iat,exp}`, with `iss: "alien-sandbox"`, a maximum
24-hour lifetime and a 4 KiB encoded size limit. Neither a policy nor a generation
appears in a proxy token. Missing policy records deny access, even if the config
contains a create default. The binding applies create defaults explicitly.

Every new connection reads the current in-memory policy. Policies support at most
256 KiB and 4096 targets. Each signed record has a format version and a version pair
of Unix milliseconds and a process-specific writer ID. Higher pairs replace older
records; equal and older pairs are no-ops. Records over sixty seconds in the future
are refused. DELETE retains a denial tombstone for at least 25 hours; an explicit
higher-version policy can recreate an identity. Memory exhaustion refuses writes
without evicting live policies.

Writes fan out over HTTP to the service's other replicas and return success only
after acknowledgements. A failed fan-out returns a retryable error; retry the same
record. Startup snapshots precede readiness, and a listed unreachable peer keeps a
new replica unready. Version summaries repair missed writes every thirty seconds.
Missing policy always denies. Total replica loss also loses deletion/version
history: agents can restore their saved signed record only while the entry is
absent. An old signed record can be replayed during that absence; replication does
not provide durable recovery of a newer restriction after total state loss. If an
older record fills the entry first, a newer management write is required to
replace it; the agent poll does not bound this replay window.

Each replica rechecks active identities every 30 seconds and closes tunnels whose
destination is no longer allowed, including after deletion or a store failure.
A stalled sweep has a 20-second deadline and closes the affected tunnels. Token
expiry and 120-second idle limits are swept every second. Activity resets the idle
clock; a one-hour absolute connection limit also applies. Admission permits at
most 32 unauthenticated connections per source IP, 128 authenticated connections
per sandbox identity and 1024 total connections. Per-identity DNS concurrency is
eight. DNS uses an async resolver, absolute names without search suffixes, and
fresh address validation before dialing. SIGTERM and SIGINT stop admission and
allow existing proxy connections up to fifteen seconds to drain.

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
`typ: "alien-egress-admin"` and claims `{iss,aud,sub,iat,exp,method,policy?,version?}`.
The lifetime is at most 60 seconds. Issuance tolerates 60 seconds of clock skew;
expiry is never extended. Proxy tokens use the same issuance allowance. `method` binds the HTTP method; for PUT,
`policy` is `sha256:<hex>` of the normalized, sorted, compact policy JSON.
It is an admin-body authenticator, never a proxy-token claim.

- `GET /v1/policies?aud=...&sub=...` returns `{version:[millis,writerId]}`,
  or `{version:null}` when absent, with an identity-bound GET grant.
- `PUT /v1/policies {record}` carries the independently signed policy record.
- `DELETE /v1/policies {record}` carries a signed deletion tombstone.
- `GET /v1/policies/record?aud=...&sub=...` returns `{record}` or 404.
- `GET /health` is unauthenticated readiness; `GET /metrics` requires an admin grant.

The record has `typ: "alien-egress-record"`, claims `{v:1,aud,sub,ver,policy}` or
`{v:1,aud,sub,ver,deleted:true}`, and no expiry. The short-lived outer admin grant
binds identity and method. The admin network must remain inaccessible to guests;
peer exchange additionally authenticates with the gateway-only replica secret.
Logs separate authorization from the close reason, with open and close events.
Refusal logs are limited to twenty per second per replica; fixed-category counters
still count every refusal. Metrics include active connections, without host or
sandbox labels. Tokens, request headers and secret values are never logged.

Optional credential rules contain `aud`, exact `host`, `port`, uppercase `method`,
`path`, `header`, `vaultBinding`, `secret`, and optional `valuePrefix`. Paths match
exactly unless they end in `/`, which selects that subtree. Ambiguous encoded paths
are refused during interception. Header values come from the deployment vault and
are never logged. The gateway replaces the matching header, verifies the upstream
TLS certificate, and does not follow redirects. An upstream service can deliberately
reflect a credential; only configure trusted origins. `upstreamCaCertificates` may
add deployment-owned trust roots encoded as base64 DER.

`POST /v1/ca {aud,sub}` with a matching admin JWS creates or returns a sandbox's
public CA certificate as `{certificate: "PEM"}`. The private CA is deterministically
derived from the gateway-only replica secret, sandbox identity and allowed host
set. All replicas derive the same CA without a separate store. The gateway needs
read permission on each configured credential vault. Deliver the public CA through
the agent's `PUT /v1/egress {token,policy,caCert?}`. Trust bundle paths
are passed to standard tools; applications with their own trust stores must import
the public CA explicitly. The CA is constrained to the audience's configured
credential host names and excludes IP certificates. Changing this host set issues
a different scoped CA, so clients must fetch and install its new public certificate.
Leaf TLS configurations are cached per sandbox and host for one hour; vault values
are cached for at most sixty seconds. Both caches retain at most 256 entries and
never serve expired entries after a dependency failure.

For a gateway-backed resource the agent starts without a token. Health reports
`awaiting-egress-policy`, and
session routes are blocked until token installation and the external path check
succeed. The agent checks supervisor TCP and UDP internet denial and runs metadata
HTTP and unmarked network probes in a child with the command's dropped identity.
It repeats these checks on authenticated Resume and waits for the SDK to deliver
the token and latest signed policy before reopening commands or forwarding.
Delivery calls authenticated `GET /health` on proxy port 3128. This verifies the identity and installed policy
without requiring guest access to admin port 3129 or dialing an upstream. On AWS the Run hook receives the AWS envelope
`{microvmId,runHookPayload}`; the payload is a JSON string `{gateway,audience}`.
The injected MicroVM ID is checked against the token subject. Elsewhere the backend
sets `ALIEN_SANDBOX_SESSION_ID`, `ALIEN_SANDBOX_EGRESS_AUDIENCE`, and
`ALIEN_SANDBOX_EGRESS_GATEWAY`. Tokens are never supplied through environment or
image state. Policy updates go directly to the gateway and succeed while an agent
is paused or unreachable. Token and record delivery happen at create, resume and
renewal. A running agent checks gateway policy state every five seconds and only
re-registers its saved record after a 404, never after a tombstone refusal. Updates affect new connections immediately and existing tunnels at the next policy sweep.

A resource declared `none` uses a loopback-only network and has no gateway or token.
The agent verifies that direct internet fails before serving. A closed runtime policy still requires the gateway and an identity token.

The privileged Linux agent installs its redirect and default-deny rules directly through
netlink in one atomic transaction. Images need neither `nft` nor `iptables`. The host
kernel must support nftables and the bootstrap process must have `NET_ADMIN`. A refused
or timed-out installation stops the agent before it binds its listener or runs image code.
Bootstrap creates no worker threads before dropping supervisor capabilities.
Loopback DNAT works on kernels without the optional redirect expression. Every
image command and exec child receives an irreversible IPv6 socket restriction,
including on kernels that expose IPv6 sockets without IPv6 nftables support.

Legacy supervisor object policies (`deny`, `allow`, and `allowDomains`) retain
their released direct path and command-UID filter, including pinned allowlisted
DNS and command IPv6 restrictions. They require no gateway token. These startup
rules also use native netlink.

Command-UID replies to external inbound TCP are denied: an incoming connection
must not become an unchecked outbound channel. Preview access uses the agent's
loopback path. System/control listeners retain their own return path. New outbound
TCP from the command UID is redirected regardless of source port.
All image commands, execs and jobs drop irreversibly to that UID. Other system
identities retain their own DNS and metadata traffic so snapshot restore can reach
the agent. No platform UID has a hardcoded bypass: a colliding command UID remains
filtered. The external network boundary remains required against a guest-root
compromise.
Proxy-aware commands bypass the proxy for localhost and loopback; declared `none`
does not advertise a proxy. Synthetic DNS replies advertise a 60-second TTL, but name mappings remain valid
until evicted for capacity. The agent retains at most 4096 names, reclaims expired entries
when full, and never reuses an
issued synthetic address, so stale answers cannot connect to a different name.
Clients must resolve again after eviction. After 131070 allocations in one agent
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

Outbound mail ports 25, 465 and 587 require an explicit destination and port in
the sandbox allowlist, such as `smtp.example.com:587`. Both `internet` and a
name-only allow entry deny those ports. All sandbox traffic shares the deployment's
outbound IP; one tenant sending abusive mail could get it blocklisted for every
tenant and customer. Explicit entries preserve legitimate mail use. Name-based
grants still require TLS SNI or HTTP Host; server-first SMTP requires an explicit
IP/CIDR and port grant. Denials include the mail-port reason in the connection log.
