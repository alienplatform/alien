# After-only runtime policy probes

This runner uses an installed Python SDK with `Sandbox.create(egress=...)` and
`Sandbox.update_egress`. It does not change the baseline Rust runner or require
merging runtime implementation changes into the conformance branch. Python 3.11
or newer runs the host scripts; the guest needs Python 3 and its standard library.
Use the deployment's normal binding environment and a wheel built from the
implementation under test. Record that wheel's revision alongside the deployed
revision. Never run this as a before measurement.

Create an uncommitted suite file with this structure, replacing every example
fixture. `bodySha256` must come from known fixture bytes, not from an arbitrary
response observed during the test. Both HTTPS names must serve their fixtures
with valid certificates. Name probes in `floor` require controlled DNS records
resolving to the indicated destination class from the gateway's resolver.
Use the actual VPC address space, an address in `10.244.0.0/16`, another private
range, and the provider's metadata address. The gateway never needs to send a
request to these destinations: it must reject CONNECT first.

```json
{
  "mode": "after",
  "revision": "DEPLOYED_COMMIT_AND_IMAGE_DIGEST",
  "binding": "internet-sandbox",
  "allowed": {"url": "https://allowed.example/probe", "bodySha256": "KNOWN_SHA256"},
  "removed": {"url": "https://removed.example/probe", "bodySha256": "KNOWN_SHA256"},
  "floor": [
    {"category": "vpc", "addressForm": "name", "url": "https://vpc.fixture.example/"},
    {"category": "vpc", "addressForm": "ip", "url": "https://10.42.3.10/"},
    {"category": "overlay", "addressForm": "name", "url": "https://overlay.fixture.example/"},
    {"category": "overlay", "addressForm": "ip", "url": "https://10.244.0.1/"},
    {"category": "private", "addressForm": "name", "url": "https://private.fixture.example/"},
    {"category": "private", "addressForm": "ip", "url": "https://192.168.1.1/"},
    {"category": "metadata", "addressForm": "name", "url": "https://metadata.fixture.example/"},
    {"category": "metadata", "addressForm": "ip", "url": "https://169.254.169.254/"}
  ],
  "natIps": ["203.0.113.4"],
  "ipEchoUrl": "https://echo.fixture.example/",
  "withoutConnectorBinding": "same-image-without-connector",
  "identityCommand": ["/path/to/trusted-token-fingerprint-observer", "{sandboxId}"]
}
```

Read `natIps` independently from the cloud NAT resource. The HTTPS echo endpoint
must return the caller's IPv4 address as plain text, optionally followed by one
newline. The report stores only the response digest and matches the exact bytes
against these independently read addresses. The ordinary fixture request is its
positive control. Do not use a static response containing the expected NAT IP.

`identityCommand` is optional: it must inspect the actual active agent token from
a trusted host observer and emit exactly its lowercase SHA-256 plus an optional
newline. It receives the sandbox ID as an argument; no shell is used. It must not
print the token or other diagnostics. Samples immediately before and after the
update prove token continuity; without both samples that row stays **unproven**.
A constant hash, a hash of the sandbox ID, or a newly minted witness token is not
valid evidence. The runner does not grant the guest access to the agent token.

`withoutConnectorBinding` is an intentionally isolated test fixture: the same
current agent/gateway configuration, but no network connector and a proven direct
internet route. Keep its setup evidence. The normal sandbox is its positive
control. Only the typed `EGRESS_PATH_UNEXPECTED` error counts as the self-check
refusal; missing bindings, validation failures, generic 503s, and provisioning
timeouts remain unproven. A successful create and command on that fixture fails.

Run and retain the log:

```sh
python3 crates/alien-sandbox-conformance/after/run.py suite.json after-run >after-run.log 2>&1
# From another terminal:
tail -f after-run.log
```

The new output directory contains `report.json` and a durable `sessions.jsonl`.
The journal records intent **before** create, including the deliberately failing
connector fixture. Cleanup runs after both success and failure. Retry interrupted
cleanup with the same binding environment:

```sh
python3 crates/alien-sandbox-conformance/after/cleanup.py after-run/sessions.jsonl
```

`terminationCallCompleted` is API acknowledgement only. Verify all journal IDs
are terminated using provider list calls before claiming cleanup. The fixture
creation/destruction belongs to the deployment owner; this script only creates
and terminates sandbox sessions. Recovery requires persistent provider session
lookup, as on AWS; it does not recover process-local Kubernetes broker claims.

A pass requires the network report **and** connection-log verification. Export
original gateway JSON events, one per line, after the probes have completed:

```sh
python3 crates/alien-sandbox-conformance/after/verify_logs.py \
  after-run/report.json gateway.jsonl --audience deployment-prefix/resource \
  --output after-run/gateway-log-check.json
```

Each completed connection must have a distinct event with the expected audience,
sandbox ID, host, port, decision, timestamp, byte counts, and duration. Both
positive controls and denied subjects need events. Floor name denials must have
`Egress denied: address is not public`; a DNS failure returning 403 cannot pass.
The verifier permits one second of clock skew. Synchronize clocks and export
complete logs; missing observations remain unproven. Log verification does not
replace controlled DNS records and setup evidence for each destination class.

The update row measures from **before** the update API call through the observed
CONNECT denial and includes command dispatch latency. It permits at most ten
seconds, uses fresh connections, and requires the removed destination to work
beforehand and the retained destination to work afterward. Existing open streams
are outside this claim. Empty allow checks both known public fixtures; it does
not claim an exhaustive test of every address and protocol.

Run local harness checks with:

```sh
python3 -m unittest discover -s crates/alien-sandbox-conformance/after -p test_after.py -v
```

These exercise real guest processes, CONNECT and TLS sockets, mutable policy,
failed controls, ignored policy, token observer validation, and missing/stale/
wrong-sandbox log events. They validate the probes, not cloud enforcement.
