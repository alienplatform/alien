# Operation permission compiler

This package runs `alien-permissions` in Node.js and browsers without a server connection.
The generated WASM and JavaScript bindings contain the Rust compiler, not a second implementation.

After changing the Rust compiler, install the `wasm-bindgen-cli` version matching `Cargo.lock`,
then run `pnpm generate` in this package. Compilation uses Rust 1.97.1 on Linux to produce the
canonical artifact. Linux hosts need that toolchain with the `wasm32-unknown-unknown` target;
other hosts need Docker, which runs the pinned Rust image and caches dependencies under `target`.
The generated files are committed so consumers need neither Rust nor Docker.

`executeOperationPermissions` accepts a serialized `operations::Request` and returns a JSON
result containing either `ok: true, value` or `ok: false, error` with an Alien structured error.
