# alien-sdk

Public Rust SDK for Alien applications. Its binding API exposes only the
application-facing `Bindings` facade and storage, KV, queue, and vault types.
Provider construction and managed resource bindings belong to
`alien-bindings`, not this crate.

The crate also owns `AlienContext` for Worker events, commands, and `waitUntil`.
Both `Bindings::from_env()` and `AlienContext::bindings()` use the same direct,
in-process binding facade on every supported platform.
