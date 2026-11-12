//! WASI preview 1 as a hook sees it. The host defines each function itself rather than using
//! wasmtime-wasi, whose defaults (a random seed per instance, the real clock) give a different
//! output on every run, and whose sync bindings block on tokio for stdio and so panic when a
//! hook prints from inside a tokio runtime.

use crate::Host;
use wasmtime::{bail, format_err, Caller, Engine, Extern, Linker, Module, Result, Val, ValType};

const WASI_MODULE: &str = "wasi_snapshot_preview1";

/// The most of a hook's stderr kept for its error, as much as of its error text: the error is
/// saved with the deployment, so a hook can't pass on much of its input through either.
const STDERR_LIMIT: usize = crate::MAX_HOOK_ERROR_BYTES;

const ERRNO_SUCCESS: i32 = 0;
const ERRNO_BADF: i32 = 8;
const ERRNO_NOSYS: i32 = 52;

const STDOUT: i32 = 1;
const STDERR: i32 = 2;

/// The functions defined below. Every other preview 1 function a module imports answers
/// ENOSYS.
const DEFINED: [&str; 10] = [
    "random_get",
    "clock_time_get",
    "environ_sizes_get",
    "args_sizes_get",
    "environ_get",
    "args_get",
    "fd_write",
    "fd_prestat_get",
    "proc_exit",
    "sched_yield",
];

/// The random source starts from the same seed in every instance, so a hook that iterates a
/// `HashMap` gives the same order on every call.
pub(crate) const RANDOM_SEED: u64 = 0x5eed;

impl Host {
    /// splitmix64, from a fixed seed.
    fn next_random_byte(&mut self) -> u8 {
        self.random = self.random.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.random;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        (z ^ (z >> 31)) as u8
    }
}

/// Builds the imports for one module, or says why the module can't have them.
pub(crate) fn linker(
    engine: &Engine,
    module: &Module,
) -> std::result::Result<Linker<Host>, String> {
    if let Some(import) = module
        .imports()
        .find(|import| import.module() != WASI_MODULE)
    {
        return Err(format!(
            "it imports `{}::{}`, and a hooks module may import only from {WASI_MODULE}",
            import.module(),
            import.name()
        ));
    }
    let mut linker = Linker::new(engine);
    define(&mut linker).map_err(|error| format!("{error:#}"))?;
    for import in module.imports() {
        if DEFINED.contains(&import.name()) {
            continue;
        }
        let Some(ty) = import.ty().func().cloned() else {
            return Err(format!(
                "it imports `{WASI_MODULE}::{}`, which is not a function",
                import.name()
            ));
        };
        let returns_errno = matches!(ty.results().collect::<Vec<_>>().as_slice(), [ValType::I32]);
        if !returns_errno {
            return Err(format!(
                "it imports `{WASI_MODULE}::{}` with a type WASI preview 1 does not have",
                import.name()
            ));
        }
        linker
            .func_new(WASI_MODULE, import.name(), ty, |_, _, results| {
                results[0] = Val::I32(ERRNO_NOSYS);
                Ok(())
            })
            .map_err(|error| format!("{error:#}"))?;
    }
    Ok(linker)
}

fn define(linker: &mut Linker<Host>) -> Result<()> {
    linker.func_wrap(
        WASI_MODULE,
        "random_get",
        |mut caller: Caller<'_, Host>, buf: i32, len: i32| -> Result<i32> {
            let (memory, host) = memory(&mut caller)?;
            for byte in slice(memory, buf, len as u32 as usize)? {
                *byte = host.next_random_byte();
            }
            Ok(ERRNO_SUCCESS)
        },
    )?;
    // Every clock reads 0, so a hook can't make its output depend on the time.
    linker.func_wrap(
        WASI_MODULE,
        "clock_time_get",
        |mut caller: Caller<'_, Host>, _clock: i32, _precision: i64, out: i32| -> Result<i32> {
            let (memory, _) = memory(&mut caller)?;
            slice(memory, out, 8)?.copy_from_slice(&0u64.to_le_bytes());
            Ok(ERRNO_SUCCESS)
        },
    )?;
    // The environment is always empty. A hook gets no arguments; a command gets the ones its run
    // passes.
    linker.func_wrap(
        WASI_MODULE,
        "environ_sizes_get",
        |mut caller: Caller<'_, Host>, count: i32, size: i32| -> Result<i32> {
            let (memory, _) = memory(&mut caller)?;
            slice(memory, count, 4)?.copy_from_slice(&0u32.to_le_bytes());
            slice(memory, size, 4)?.copy_from_slice(&0u32.to_le_bytes());
            Ok(ERRNO_SUCCESS)
        },
    )?;
    linker.func_wrap(
        WASI_MODULE,
        "environ_get",
        |_: Caller<'_, Host>, _: i32, _: i32| ERRNO_SUCCESS,
    )?;
    linker.func_wrap(
        WASI_MODULE,
        "args_sizes_get",
        |mut caller: Caller<'_, Host>, count: i32, size: i32| -> Result<i32> {
            let (memory, host) = memory(&mut caller)?;
            let total: usize = host.args.iter().map(|arg| arg.len() + 1).sum();
            let (count_value, total) = (host.args.len() as u32, total as u32);
            slice(memory, count, 4)?.copy_from_slice(&count_value.to_le_bytes());
            slice(memory, size, 4)?.copy_from_slice(&total.to_le_bytes());
            Ok(ERRNO_SUCCESS)
        },
    )?;
    // Writes each argument, NUL-terminated, into `buf`, and its address into `argv`.
    linker.func_wrap(
        WASI_MODULE,
        "args_get",
        |mut caller: Caller<'_, Host>, argv: i32, buf: i32| -> Result<i32> {
            let (memory, host) = memory(&mut caller)?;
            let mut at = buf as u32;
            for (i, arg) in host.args.iter().enumerate() {
                let entry = (argv as u32)
                    .checked_add(i as u32 * 4)
                    .ok_or_else(|| format_err!("argv outside memory"))?;
                slice(memory, entry as i32, 4)?.copy_from_slice(&at.to_le_bytes());
                let bytes = slice(memory, at as i32, arg.len() + 1)?;
                bytes[..arg.len()].copy_from_slice(arg);
                bytes[arg.len()] = 0;
                at = at
                    .checked_add(arg.len() as u32 + 1)
                    .ok_or_else(|| format_err!("arguments outside memory"))?;
            }
            Ok(ERRNO_SUCCESS)
        },
    )?;
    // Stdout is dropped for a hook and kept, up to its limit, for a command. Stderr, up to its
    // limit, goes into the hook's error, which is where a panic message lands.
    linker.func_wrap(
        WASI_MODULE,
        "fd_write",
        |mut caller: Caller<'_, Host>,
         fd: i32,
         iovs: i32,
         iovs_len: i32,
         written: i32|
         -> Result<i32> {
            if fd != STDOUT && fd != STDERR {
                return Ok(ERRNO_BADF);
            }
            let (memory, host) = memory(&mut caller)?;
            let mut total = 0u32;
            for i in 0..iovs_len as u32 {
                let at = (iovs as u32)
                    .checked_add(
                        i.checked_mul(8)
                            .ok_or_else(|| format_err!("iovec outside memory"))?,
                    )
                    .ok_or_else(|| format_err!("iovec outside memory"))?;
                let ptr = read_u32(memory, at)?;
                let len = read_u32(memory, at.wrapping_add(4))?;
                let bytes = slice(memory, ptr as i32, len as usize)?;
                let (kept, limit) = if fd == STDERR {
                    (&mut host.stderr, STDERR_LIMIT)
                } else {
                    (&mut host.stdout, host.stdout_limit)
                };
                let room = limit.saturating_sub(kept.len());
                kept.extend_from_slice(&bytes[..bytes.len().min(room)]);
                total = total.saturating_add(len);
            }
            slice(memory, written, 4)?.copy_from_slice(&total.to_le_bytes());
            Ok(ERRNO_SUCCESS)
        },
    )?;
    // No preopened directory, so the standard library finds no files to open.
    linker.func_wrap(
        WASI_MODULE,
        "fd_prestat_get",
        |_: Caller<'_, Host>, _: i32, _: i32| ERRNO_BADF,
    )?;
    linker.func_wrap(
        WASI_MODULE,
        "proc_exit",
        |_: Caller<'_, Host>, code: i32| -> Result<()> { Err(ProcExit(code).into()) },
    )?;
    linker.func_wrap(WASI_MODULE, "sched_yield", |_: Caller<'_, Host>| {
        ERRNO_SUCCESS
    })?;
    Ok(())
}

/// A call to `proc_exit`, which ends the instance: a hook fails with it, and a command run ends
/// with its code.
#[derive(Debug)]
pub(crate) struct ProcExit(pub(crate) i32);

impl std::fmt::Display for ProcExit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the hook called proc_exit({})", self.0)
    }
}

impl std::error::Error for ProcExit {}

fn memory<'a>(caller: &'a mut Caller<'_, Host>) -> Result<(&'a mut [u8], &'a mut Host)> {
    match caller.get_export("memory") {
        Some(Extern::Memory(memory)) => Ok(memory.data_and_store_mut(caller)),
        _ => bail!("the module exports no memory"),
    }
}

fn slice(memory: &mut [u8], ptr: i32, len: usize) -> Result<&mut [u8]> {
    let start = ptr as u32 as usize;
    start
        .checked_add(len)
        .and_then(|end| memory.get_mut(start..end))
        .ok_or_else(|| format_err!("the hook passed a pointer outside its memory"))
}

fn read_u32(memory: &mut [u8], at: u32) -> Result<u32> {
    let bytes = slice(memory, at as i32, 4)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}
