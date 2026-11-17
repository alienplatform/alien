//! Hand-written hooks modules for tests, loaded the way a loader loads an extension's bundle.

use sha2::Digest;

/// A module for `extension` that declares `hooks`, a JSON list of hook entries, and answers
/// every call with the WebAssembly in `call`.
pub fn hooks_module(extension: &str, hooks: &str, call: &str) -> alien_hooks_runtime::HookModule {
    let manifest = format!(r#"{{"extension":"{extension}","hooks":{hooks}}}"#);
    let packed = 16u64 | ((manifest.len() as u64) << 32);
    let wasm = wat::parse_str(format!(
        r#"(module
            (memory (export "memory") 1)
            (data (i32.const 16) "{}")
            (func (export "alien_hooks_abi_v1"))
            (func (export "alien_alloc") (param i32) (result i32) i32.const 1024)
            (func (export "alien_hooks_manifest") (result i64) i64.const {packed})
            (func (export "alien_hooks_call") (param i32 i32 i32 i32) (result i64) {call}))"#,
        manifest.replace('"', "\\\"")
    ))
    .unwrap();
    alien_hooks_runtime::Runtime::new()
        .unwrap()
        .load(&wasm, &hex::encode(sha2::Sha256::digest(&wasm)))
        .unwrap()
}

/// The hooks module of an extension whose hooks don't matter to the test: it declares none.
pub fn no_hooks(extension: &str) -> alien_hooks_runtime::HookModule {
    hooks_module(extension, "[]", "i64.const 0")
}
