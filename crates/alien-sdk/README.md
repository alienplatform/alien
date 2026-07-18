# alien-sdk

Public Rust SDK for Alien applications. Its binding API exposes only the
application-facing `Bindings` facade and storage, KV, queue, and vault types.
Provider construction and managed resource bindings belong to
`alien-bindings`, not this crate.

The `worker` module owns `worker::AlienContext` for Worker events, commands, and `waitUntil`.
Both `Bindings::from_env()` and `worker::AlienContext::bindings()` use the same direct,
in-process binding facade on every supported platform.

Worker applications must enable the `worker` Cargo feature:

```toml
[dependencies]
alien-sdk = { version = "1.14.1", features = ["worker"] }
```

The feature is intentionally not enabled by default, so Containers and Daemons
using only direct bindings do not depend on the Worker protocol.
