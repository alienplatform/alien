# Sandbox egress conformance

This crate observes real network operations from a sandbox command. The same
probe runs against a controlled endpoint from a positive-control sandbox before
its result is interpreted. A failed control means **unproven**, never pass.

The initial transport probes cover direct TCP, DNS resolution, HTTP, HTTPS,
explicit HTTP proxies, an independently selected TLS SNI, and a forged HTTP
Host. They use Python 3's standard library in the guest. One five-second deadline
covers resolution, connection, TLS, and response reading. Response bodies are
bounded at 1 MiB and reported as a length and SHA-256 digest; credentials and
body contents are not returned.

`Expectation::Response` requires the fixture's known response digest. Network
denials require a reachable control listener. An origin's HTTP 403 proves the
network was reachable; only a CONNECT rejection is classified as a proxy
denial. DNS and TLS errors are inconclusive for a network-denial claim.

Run the probe transport checks with:

```sh
cargo nextest run -p alien-sandbox-conformance --locked
cargo clippy -p alien-sandbox-conformance --all-targets --all-features --locked -- -D warnings
```

These checks exercise loopback listeners. They do not prove a backend's egress
policy. Live qualification additionally needs deployed subject/control
sandboxes, controlled public and private listeners, authoritative DNS query
logs, gateway connection logs, and the deployment's independently read NAT IP.
DNS resolution failure alone cannot prove that a query did not escape.
A timeout, reset or route error after the TCP connection is established is
reported as `incompleteAfterConnect`: the network carried the traffic, so it
never proves a denial.
The `dnsQuery` probe sends an A query directly to a numeric `server` and `port`,
with a unique `host` under a controlled zone. It bypasses the configured resolver
and proxy, checks the reply's source, transaction, question and authoritative flag, and treats even
NXDOMAIN as reachability. Compare authoritative logs for unique control and
subject labels; a timeout alone does not prove non-exfiltration.

Build the live runner with the deployment's backend feature (`aws`, `gcp`,
`azure`, `kubernetes`, or `local`). It reads the standard deployment bindings
from the environment:

```sh
cargo run -p alien-sandbox-conformance --features aws -- run suite.json sessions.jsonl
cargo run -p alien-sandbox-conformance --features aws -- cleanup sessions.jsonl
```

The suite names distinct `controlBinding` and `subjectBinding`, identifies its
`backend`, `revision`, and `mode`, and supplies `cases`. Each case has a `name`,
`control` and `subject` probe, and an `expected` result. For example, a direct
TCP denial uses these fields:

```json
{
  "name": "direct public TCP",
  "control": { "kind": "tcp", "host": "controlled.example", "port": 443 },
  "subject": { "kind": "tcp", "host": "controlled.example", "port": 443 },
  "expected": { "kind": "networkDenied" }
}
```

The runner creates both sessions, retains their provider handles, and attempts
termination on success and failure. It persists each session ID immediately in
a new journal and refuses to overwrite a previous run. A non-passing verdict
returns a failing exit code. The journal remains available for recovery.
`terminationCallsCompleted` only confirms successful termination calls; verify
resource deletion with backend list calls before claiming cleanup. Kubernetes
recovery after process exit requires its broker because its bindings currently
keep session claims in memory; the recovery command refuses to silently skip
those sessions.

Check a Kubernetes cluster's actual NetworkPolicy enforcement with:

```sh
python3 crates/alien-sandbox-conformance/cni_probe.py --kubeconfig /path/to/config
```

This creates a fresh namespace containing a controlled HTTP listener, an
unrestricted client, and a client selected by a deny-all egress policy. Three
paired requests must return the exact fixture bytes from the control and fail
at the network layer from the denied client. Failed controls are unproven;
reachable denied clients fail. The command deletes its namespace before
reporting success and returns a nonzero exit status for failed or unproven
checks. `--image` can select an available Python 3 image.

The report includes pod placements: this checks the selected nodes and address
family, not every node or both IP families. It does not establish sandbox
runtime isolation or qualify the sandbox's own network rules. Installation and
an accepted NetworkPolicy object alone do not establish enforcement.

After-only runtime policy, NAT, gateway-log and connector-refusal probes live in
[`after/`](after/README.md). They require the implementation under test’s Python
SDK and preserve this baseline runner unchanged.

Run the image boot/USER fixture through real runtime bindings with:

```sh
cargo run -p alien-sandbox-conformance --features gcp -- \
  entrypoint entrypoint-suite.json sessions.jsonl
```

The suite contains `backend`, `revision`, `binding`, `imageUid`, `execUid`, and
optional `metadataBackend` (`aws`, `gcp`, or `azure`). Build the image from
[`entrypoint/`](entrypoint/README.md), using the same metadata backend at build
time, then pass it through the normal backend image assembler. The runner creates
one session, persists its handle, and runs the fixture inspector as the first user
exec. It reports the observed identities and the checker's exit status, then
terminates the session even on failure. A failed checker returns a failing process
status. As with the network suite, verify deletion through provider inventory and
retain the journal for recovery; termination calls alone are not proof of absence.

For before/after qualification, keep the base fixture and checker identical while
assembling once with the parent builder/agent and once with the changed
builder/agent. A before run must reach the checker and fail its assertions; a
failed deployment or readiness probe does not establish the intended regression.
