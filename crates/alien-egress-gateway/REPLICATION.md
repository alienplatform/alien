# Policy replication protocol

The admin and peer listener uses the configured `admin` address. The proxy
listener remains `listen`. Local tests may configure
`peers: {"static": ["127.0.0.1:3129", "127.0.0.1:3139"]}` with the other replicas'
admin addresses (exclude the current replica). Service discovery uses
`peers: {"dns": "gateway.svc", "port": 3129}`. Omit `peers` for a single replica.
`replicaSecret` is a base64-encoded 32-byte secret shared only by gateway replicas.
The optional `policyMemoryBytes` overrides half the finite cgroup v2 `memory.max`
limit (256 MiB for the default 512 MiB container). Without a finite limit or explicit override,
startup fails. Local processes in an unbounded host must set an override.

A record is a compact Ed25519 JWS with `typ: "alien-egress-record"` and the same
`kid` used by admin/proxy signing. Its claims are `{v:1,aud,sub,ver:[unixMillis,
writerId],policy}` or `{v:1,aud,sub,ver:[unixMillis,writerId],deleted:true}`.
There is no expiry. `writerId` is 32 hexadecimal characters. `record::Claims`,
`record::Version`, and `record::compare_versions` expose the shared format and
production ordering. The gateway checks record signatures independently of
short-lived admin authorization. Unknown formats and timestamps over 60 seconds
in the future are rejected.

All JSON uses the following wire forms:

- `PUT /v1/policies` and `DELETE /v1/policies`: `{"record":"<compact JWS>"}`.
  Authorization is the existing `alien-egress-admin` bearer JWS with `iss`,
  `aud`, `sub`, `method`, `iat`, `exp` (at most 60 seconds). The signed record
  supplies policy and version authorization. Success is 204.
- `GET /v1/policies/record?aud=...&sub=...`: identity-bound admin GET grant;
  returns `{"record":"<compact JWS>"}` or 404 when absent. Tombstones return 200.
- `GET /v1/policies?aud=...&sub=...`: same GET grant; returns
  `{"version":[millis,writerId]}` or `{"version":null}` when absent.
- `POST /v1/replicas/write` and `/register`: `{"record":"<compact JWS>"}`;
  204 after merge. Register only fills an absent identity.
- `POST /v1/replicas/summary`: body `null`; returns an array of
  `{aud,sub,ver:[millis,writerId]}`.
- `POST /v1/replicas/pull` (alias `/snapshot`): body is an array of known summaries;
  returns an array of compact JWS strings newer than those summaries. An empty
  array starts a full snapshot. Responses are bounded to 8 MiB; repeat with the
  updated summary until the array is empty. Both sides stream records individually;
  snapshot endpoints work before readiness.
- `GET /health`: 200 only after synchronization, otherwise 503.

For the configured peer service, a DNS NXDOMAIN or NODATA answer means no peers
are currently published. An empty replica can then become ready so service
discovery and agent re-registration can recover after total loss. The configured
resolver supplies membership, including negative answers; an SOA record is not
required. DNS SERVFAIL, REFUSED and transport timeouts remain failures and keep
an unsynchronized replica unready. A listed but unreachable peer also blocks
readiness.

Peer POSTs carry `x-alien-replica-time` (Unix seconds) and
`x-alien-replica-signature` (standard base64 HMAC-SHA256). HMAC input is
`alien-egress-replica-v1\n`, method, newline, exact path, newline, timestamp as
8 big-endian bytes, then the SHA256 digest of the exact request body. The timestamp
must be within 60 seconds. Every received record also requires its own Ed25519
signature; peer authentication does not replace record verification.

The proxy listener accepts `GET /health` with a live `Proxy-Authorization: Bearer`
sandbox token: 200 means an existing policy (including deny-all), 403 means a
tombstone, and 404 means no entry. `PUT /v1/policies/register` on that listener
accepts `{"record":"<compact JWS>"}` with the same token and bounded
`Content-Length`. It only fills absence, including on peers: 204 means the saved
record is current; 409 means an existing entry prevented replacement; 503 means
replication failed. The agent never registers after a 403.

Agent delivery is `PUT /v1/egress` with `{token,policy:"<compact JWS>",caCert?}`;
the `updateEgress` envelope carries the same fields. Resume checks the physical
network and verifies the installed identity against the current gateway policy.
It reopens commands on success regardless of hook/delivery ordering; a rejected
identity stays closed until renewal.
For a running policy update, `{policy:"<compact JWS>"}` refreshes only the saved
record, preserving the installed token. It requires an existing identity that
the gateway still accepts, and cannot release a gate closed by a rejected identity or failed path check.
A running agent checks gateway state every five seconds, re-registering only on
404. The saved record and token remain in supervisor memory. Tombstones expire when `now > ver.millis + 25h + 60s`, independently on each
replica. Already-expired tombstones are ignored and never forwarded; catch-up
cannot extend their life. The 25-hour retention exceeds the maximum 24-hour token
lifetime, and a deleted sandbox never gets a renewed token. Therefore, even if an
old policy reappears after its tombstone is gone, no valid token can use it. Live
records are never evicted.

Total loss of every replica also loses version and deletion history. A live
identity token can therefore re-register an older signed policy while the entry
is absent. This protocol cannot distinguish that record from the latest one;
the independent restart tests measure this window. If an older record fills the
entry first, absent-only recovery cannot replace it: stale authorization remains
until an explicit newer management write. The five-second agent poll is not a
revocation bound after total state loss. Agentless clients
remain denied until their application publishes a policy again.

`maxConnections` limits all open proxy connections (default 2048, configurable
between 1 and 65536). Excess connections are refused before tunnel buffers are
allocated, with a rate-limited `global connection capacity` log and denial counter.
Changing the limit requires a restart. Per-source and per-identity limits still apply.

## Release memory measurements

Measured on 2026-10-18 with the x86_64 musl release binary at `fe53b9fab`,
inside a read-only, capability-free container limited to 512 MiB. The record
budget was derived from cgroup `memory.max`, with no configuration override.
Peak RSS is the process's Linux `VmHWM`; it includes allocator overhead but not
kernel socket memory. These controlled local loads are sizing evidence, not a
production throughput guarantee.

| Load | Result | Peak RSS |
| --- | --- | ---: |
| 10,000 sandboxes, each with a 1 KiB policy | 10,000 accepted | 189,104,128 bytes (180.3 MiB) |
| Startup snapshot of those 10,000 records | Ready after 3.03 s; all versions matched | 102,621,184 bytes (97.9 MiB) |
| Source serving that snapshot | All records retained | 192,090,112 bytes (183.2 MiB) |
| 1,000 attempts with 256 KiB policies | 127 accepted; 873 returned `SANDBOX_EGRESS_POLICY_STORE_FULL` | 226,955,264 bytes (216.4 MiB) |
| Startup snapshot of the full large-policy store | All 127 accepted records restored in 1.90 s with the same memory limit | 251,224,064 bytes (239.6 MiB) |
| 2,000 open CONNECT tunnels with traffic | All tunnels exchanged data | 93,646,848 bytes (89.3 MiB) |
| Full policy store plus 2,000 tunnels with traffic | All tunnels exchanged data | 307,109,888 bytes (292.9 MiB) |
| Full policy store plus 2,048 tunnels | Connection 2,049 refused and logged | 309,084,160 bytes (294.8 MiB) |

No process was OOM-killed. The combined peak leaves about 217 MiB below the
container limit; the measurements do not support reducing it to 256 MiB. Record
accounting charges twice the retained allocations to cover measured allocator
size classes and fragmentation. It counts both decoded policy copies and the
signed record. Retained strings and vectors have their spare capacity removed
before charging, so identical records cost the same on every ingestion path.
A full store refuses writes without evicting a live policy. The 2,048 default
connection cap bounds tunnel memory separately; increasing it or overriding
the record budget requires sizing for the deployment's workload.
