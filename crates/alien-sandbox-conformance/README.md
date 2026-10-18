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
