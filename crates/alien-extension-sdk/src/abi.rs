//! The module side of the hooks ABI, version 1, which `export!` wires to the module's exports.
//!
//! The host writes a hook's name and input into memory from `alien_alloc` and calls
//! `alien_hooks_call`. A result packs the output's pointer in bits 0 to 31 and its length in bits
//! 32 to 62; bit 63 set means the output is the hook's error, a `HookError` in JSON. The host runs every call in
//! a fresh instance, so nothing the module allocates is ever freed.

use crate::Extension;
use std::io::Write;
use std::sync::Once;

pub fn alloc(len: i32) -> i32 {
    let mut buffer = Vec::<u8>::with_capacity(len as usize);
    let ptr = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    ptr as i32
}

pub fn manifest(extension: fn() -> Extension) -> i64 {
    install_panic_hook();
    pack(
        extension()
            .manifest()
            .and_then(|manifest| crate::write_output(&manifest)),
    )
}

/// # Safety
///
/// Both buffers come from `alloc`, and the host wrote `name_len` and `input_len` bytes into them.
pub unsafe fn call(
    extension: fn() -> Extension,
    name_ptr: i32,
    name_len: i32,
    input_ptr: i32,
    input_len: i32,
) -> i64 {
    install_panic_hook();
    // SAFETY: as the caller promises.
    let (name, input) = unsafe { (take(name_ptr, name_len), take(input_ptr, input_len)) };
    let result = match String::from_utf8(name) {
        Ok(name) => extension().call(&name, &input),
        Err(_) => Err("the hook name is not UTF-8".into()),
    };
    pack(result)
}

/// A panic aborts the module, so the hook's error has to be written before: the host keeps the
/// module's stderr and puts it in the error it reports.
fn install_panic_hook() {
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            let _ = writeln!(std::io::stderr(), "the hook panicked: {info}");
        }));
    });
}

fn pack(result: Result<Vec<u8>, alien_core::hooks::HookError>) -> i64 {
    let (output, error) = match result {
        Ok(bytes) => (bytes, false),
        // The host reads the error's code, message and retry flag from its JSON.
        Err(error) => (
            serde_json::to_vec(&error).unwrap_or_else(|_| error.message.into_bytes()),
            true,
        ),
    };
    let packed = output.as_ptr() as u32 as u64 | (output.len() as u64) << 32 | (error as u64) << 63;
    std::mem::forget(output);
    packed as i64
}

/// # Safety
///
/// `ptr` came from `alloc(len)` and holds `len` initialized bytes.
unsafe fn take(ptr: i32, len: i32) -> Vec<u8> {
    // SAFETY: `alloc` leaked a `Vec<u8>` with this pointer and capacity, which the host filled.
    unsafe { Vec::from_raw_parts(ptr as *mut u8, len as usize, len as usize) }
}
