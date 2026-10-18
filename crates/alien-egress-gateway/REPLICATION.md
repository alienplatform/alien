# Policy replication protocol

The admin and peer listener uses the configured `admin` address. The proxy
listener remains `listen`. Local tests may configure
`peers: {"static": ["127.0.0.1:3129", "127.0.0.1:3139"]}` with the other replicas'
admin addresses (exclude the current replica). Service discovery uses
`peers: {"dns": "gateway.svc", "port": 3129}`. Omit `peers` for a single replica.
`replicaSecret` is a base64-encoded 32-byte secret shared only by gateway replicas.
The configured `policyMemoryBytes` defaults to 512 MiB.

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
  updated summary until the array is empty. Snapshot endpoints work before readiness.
- `GET /health`: 200 only after synchronization, otherwise 503.

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
network and waits for a fresh delivery before allowing commands or recovery.
A running agent checks gateway state every five seconds, re-registering only on
404. The saved record and token remain in supervisor memory. Tombstones expire
at least 25 hours after local acceptance; live records are never evicted.

Total loss of every replica also loses version and deletion history. A live
identity token can therefore re-register an older signed policy while the entry
is absent. This protocol cannot distinguish that record from the latest one;
the independent restart tests measure the recovery window. Agentless clients
remain denied until their application publishes a policy again.
