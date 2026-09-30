# Operation permission compiler

This package runs `alien-permissions` in Node.js and browsers without a server connection.
The generated WASM and JavaScript bindings contain the Rust compiler, not a second implementation.

After changing the Rust compiler, install Rust 1.97.1 with the `wasm32-unknown-unknown` target and the
`wasm-bindgen-cli` version matching `Cargo.lock`, then run `pnpm generate` in this package.
The generated files are committed so consumers do not need a Rust toolchain.

`executeOperationPermissions` accepts a serialized `operations::Request` and returns a JSON
result containing either `ok: true, value` or `ok: false, error` with an Alien structured error.
