# Policy replication protocol

The admin and peer listener uses the configured `listenAdmin` address. The proxy
listener remains `listenProxy`. Local tests may configure
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
  array requests a full snapshot. Snapshot endpoints work before readiness.
- `GET /health`: 200 only after synchronization, otherwise 503.

Peer POSTs carry `x-alien-replica-time` (Unix seconds) and
`x-alien-replica-signature` (standard base64 HMAC-SHA256). HMAC input is
`alien-egress-replica-v1\n`, method, newline, exact path, newline, timestamp as
8 big-endian bytes, then the SHA256 digest of the exact request body. The timestamp
must be within 60 seconds. Every received record also requires its own Ed25519
signature; peer authentication does not replace record verification.

This initial surface is available for integration testing. Agent recovery and
bounded tombstone expiry are being completed before release qualification.
