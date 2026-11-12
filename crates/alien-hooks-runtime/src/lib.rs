//! Runs an Alien extension's hooks module.
//!
//! An extension's modifiers, emitters, importers, links and interceptors build to one
//! `wasm32-wasip1` reactor module. [`Runtime::load`] checks its sha256 and its ABI, compiles it
//! once per digest, and reads the hooks it declares. [`HookModule::call`] then runs one hook by
//! name in a fresh instance, with no network, no files, no environment, a fixed clock and a
//! seeded random source, so the same input gives the same output on every run, and with limits
//! on time, memory and output.
//!
//! The module ABI, version 1. The module exports:
//!
//! - `memory`, its linear memory.
//! - `_initialize: () -> ()`, run once per instance before anything else, when present.
//! - `alien_hooks_abi_v1: () -> ()`, the version marker. A later ABI exports another name.
//! - `alien_alloc: (len: i32) -> i32`, memory for the host to write a hook's name and input into.
//! - `alien_hooks_manifest: () -> i64`, the [`HooksManifest`] as JSON.
//! - `alien_hooks_call: (name_ptr: i32, name_len: i32, input_ptr: i32, input_len: i32) -> i64`,
//!   which runs the hook named by the UTF-8 name on the input.
//!
//! An `i64` result packs the output's pointer in bits 0 to 31 and its length in bits 32 to 62.
//! Bit 63 set means the output is the hook's UTF-8 error message instead. The module imports
//! only from `wasi_snapshot_preview1`.

mod build;
mod error;
mod wasi;

pub use alien_core::hooks::{HookEntry, HookKind, HooksManifest};
pub use build::{build_module, build_test_modules, test_binaries, TestBinary, HOOKS_PROFILE};
pub use error::{ErrorData, Result};

use alien_core::hooks::HookError;
use alien_error::{AlienError, Context, IntoAlienError};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};
use wasmtime::{
    Config, Engine, EngineWeak, Instance, InstancePre, Memory, Module, ResourceLimiter, Store,
    Trap, TypedFunc,
};

const ABI_MARKER: &str = "alien_hooks_abi_v1";

/// One epoch tick. A deadline is counted in ticks, so a tick is the precision of every deadline.
const EPOCH_TICK: Duration = Duration::from_millis(1);

/// Rust's own stack lives in linear memory; this bounds only the native stack wasm frames use.
const MAX_WASM_STACK: usize = 512 << 10;

/// Far above the function count of any hooks module, and small enough that a module can't
/// spend the host's memory on table elements.
const TABLE_ELEMENTS_LIMIT: usize = 100_000;

/// What one hook call may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The most linear memory the instance may grow to
    pub memory_bytes: usize,
    /// The largest output the hook may return
    pub output_bytes: usize,
    /// How long the call may run before it is stopped
    pub deadline: Duration,
}

impl Limits {
    /// For modifiers and emitters, which run while Alien builds a template.
    pub const MODIFIER_OR_EMITTER: Limits = Limits {
        memory_bytes: 64 << 20,
        output_bytes: 16 << 20,
        deadline: Duration::from_secs(2),
    };

    /// For interceptors and importers, which run inside a cloud call or an import.
    pub const INTERCEPTOR_OR_IMPORTER: Limits = Limits {
        memory_bytes: 64 << 20,
        output_bytes: 16 << 20,
        deadline: Duration::from_millis(100),
    };

    /// The limits Alien calls a hook of `kind` with.
    pub fn for_kind(kind: HookKind) -> Limits {
        match kind {
            HookKind::CloudformationModifier
            | HookKind::TerraformModifier
            | HookKind::HelmModifier
            | HookKind::CloudformationEmitter
            | HookKind::TerraformEmitter
            | HookKind::HelmEmitter => Limits::MODIFIER_OR_EMITTER,
            HookKind::Importer
            | HookKind::Link
            | HookKind::AwsInterceptor
            | HookKind::GcpInterceptor
            | HookKind::AzureInterceptor
            | HookKind::KubernetesInterceptor => Limits::INTERCEPTOR_OR_IMPORTER,
        }
    }

    /// For a whole run of a hooks crate's test binary, as [`CommandModule::run`] runs it. A
    /// command's output is what it writes to stdout, and the run keeps up to `output_bytes` of
    /// it.
    pub const TEST_RUN: Limits = Limits {
        memory_bytes: 64 << 20,
        output_bytes: 1 << 20,
        deadline: Duration::from_secs(60),
    };
}

/// The largest hooks module the runtime compiles: the cap a bundle store keeps on one object.
pub const MAX_MODULE_BYTES: usize = 64 << 20;

/// How many compiled modules a runtime keeps. A manager serving many releases loads many
/// modules over its life; past this, the one loaded longest ago is compiled again when needed.
const MAX_CACHED_MODULES: usize = 64;

/// Compiles and runs hooks modules. A process creates one and shares it: every module compiles
/// against its engine, and one thread per runtime advances the clock that deadlines read.
#[derive(Clone)]
pub struct Runtime {
    engine: Engine,
    /// Compiled modules by sha256, oldest first. A digest names the module's exact bytes, so an
    /// entry can't go stale.
    modules: Arc<Mutex<VecDeque<(String, HookModule)>>>,
}

impl Runtime {
    /// Starts the engine and its epoch thread.
    pub fn new() -> Result<Self> {
        let mut config = Config::new();
        config
            .epoch_interruption(true)
            .max_wasm_stack(MAX_WASM_STACK)
            // NaNs and relaxed SIMD then give the same bits on arm64 and x86_64, so a hook's
            // output does not depend on the machine that runs it.
            .cranelift_nan_canonicalization(true)
            .relaxed_simd_deterministic(true);
        let engine = Engine::new(&config).map_err(|error| {
            AlienError::new(ErrorData::RuntimeStartFailed {
                message: format!("{error:#}"),
            })
        })?;
        let epoch = engine.weak();
        thread::Builder::new()
            .name("alien-hooks-epoch".to_string())
            .spawn(move || advance_epoch(epoch))
            .into_alien_error()
            .context(ErrorData::RuntimeStartFailed {
                message: "the epoch thread did not start".to_string(),
            })?;
        Ok(Self {
            engine,
            modules: Arc::default(),
        })
    }

    /// Loads a WASI command module, such as a hooks crate's test binary built for wasm32-wasip1,
    /// to run in the sandbox hooks run in.
    pub fn load_command(&self, wasm: &[u8]) -> Result<CommandModule> {
        let refused = |reason: String| {
            AlienError::new(ErrorData::ModuleRefused {
                sha256: hex::encode(Sha256::digest(wasm)),
                reason,
            })
        };
        let module = Module::new(&self.engine, wasm)
            .map_err(|error| refused(format!("it is not a valid WebAssembly module: {error:#}")))?;
        if module.get_export("_start").is_none() {
            return Err(refused(
                "it is not a WASI command module: it has no `_start` export".to_string(),
            ));
        }
        let pre = wasi::linker(&self.engine, &module)
            .and_then(|linker| {
                linker
                    .instantiate_pre(&module)
                    .map_err(|error| format!("{error:#}"))
            })
            .map_err(refused)?;
        Ok(CommandModule { pre })
    }

    /// Loads a hooks module: checks that its sha256 is `expected_sha256`, then compiles it,
    /// checks its ABI and reads its manifest, unless a module with that digest is already loaded.
    pub fn load(&self, wasm: &[u8], expected_sha256: &str) -> Result<HookModule> {
        let sha256 = hex::encode(Sha256::digest(wasm));
        if !sha256.eq_ignore_ascii_case(expected_sha256) {
            return Err(AlienError::new(ErrorData::ModuleDigestMismatch {
                expected_sha256: expected_sha256.to_string(),
                actual_sha256: sha256,
            }));
        }
        if let Some(module) = cached(&self.modules(), &sha256) {
            return Ok(module);
        }
        if wasm.len() > MAX_MODULE_BYTES {
            return Err(AlienError::new(ErrorData::ModuleRefused {
                sha256,
                reason: format!(
                    "it is {} bytes, more than the {MAX_MODULE_BYTES} a hooks module may take",
                    wasm.len()
                ),
            }));
        }
        let module = self.compile(wasm).map_err(|reason| {
            AlienError::new(ErrorData::ModuleRefused {
                sha256: sha256.clone(),
                reason,
            })
        })?;
        let mut modules = self.modules();
        // Two threads that load the same new module both compile it; the first one stored wins.
        if let Some(module) = cached(&modules, &sha256) {
            return Ok(module);
        }
        if modules.len() >= MAX_CACHED_MODULES {
            modules.pop_front();
        }
        modules.push_back((sha256, module.clone()));
        Ok(module)
    }

    fn modules(&self) -> std::sync::MutexGuard<'_, VecDeque<(String, HookModule)>> {
        // The map holds no invariant a panic could break halfway.
        self.modules.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn compile(&self, wasm: &[u8]) -> std::result::Result<HookModule, String> {
        let module = Module::new(&self.engine, wasm)
            .map_err(|error| format!("it is not a valid WebAssembly module: {error:#}"))?;
        if module.get_export(ABI_MARKER).is_none() {
            return Err(format!(
                "it is not an Alien hooks module, or it was built for another hooks ABI: it has no `{ABI_MARKER}` export"
            ));
        }
        let pre = wasi::linker(&self.engine, &module)?
            .instantiate_pre(&module)
            .map_err(|error| format!("{error:#}"))?;
        let manifest = run(&pre, &Limits::MODIFIER_OR_EMITTER, |store, abi| {
            abi.manifest.call(store, ())
        })
        .map_err(|failure| format!("its manifest could not be read: {failure}"))?;
        let manifest = serde_json::from_slice(&manifest)
            .map_err(|error| format!("its manifest is not valid: {error}"))?;
        Ok(HookModule { manifest, pre })
    }
}

/// The compiled module with `sha256`, if the runtime keeps it.
fn cached(modules: &VecDeque<(String, HookModule)>, sha256: &str) -> Option<HookModule> {
    modules
        .iter()
        .find(|(digest, _)| digest == sha256)
        .map(|(_, module)| module.clone())
}

/// Advances the engine's epoch once per tick until the engine is gone. A sleep of one tick lasts
/// longer than a tick (about 1.5 ms on macOS), so it counts wall-clock time instead of its own
/// sleeps, and a deadline in ticks stays a deadline in milliseconds.
fn advance_epoch(engine: EngineWeak) {
    let started = Instant::now();
    let mut ticks: u128 = 0;
    loop {
        thread::sleep(EPOCH_TICK);
        let Some(engine) = engine.upgrade() else {
            return;
        };
        let due = started.elapsed().as_nanos() / EPOCH_TICK.as_nanos();
        while ticks < due {
            engine.increment_epoch();
            ticks += 1;
        }
    }
}

/// A loaded hooks module. Cloning it is cheap, and every clone shares the compiled module.
#[derive(Clone)]
pub struct HookModule {
    /// The hooks the module declares
    pub manifest: HooksManifest,
    pre: InstancePre<Host>,
}

impl HookModule {
    /// Runs the hook named `hook` on `input` in a fresh instance and returns its output.
    ///
    /// The call blocks its thread for up to `limits.deadline`. From async code, call it through
    /// `tokio::task::spawn_blocking`, or `block_in_place` on a multi-thread runtime.
    pub fn call(&self, hook: &str, input: &[u8], limits: &Limits) -> Result<Vec<u8>> {
        run(&self.pre, limits, |store, abi| {
            let name = abi.write(store, hook.as_bytes())?;
            let input = abi.write(store, input)?;
            abi.call.call(store, (name.0, name.1, input.0, input.1))
        })
        .map_err(|failure| {
            let extension = self.manifest.extension.clone();
            let hook = hook.to_string();
            // A hook says whether another try may pass; Alien keeps that, and its code.
            let retryable = matches!(&failure, Failure::Hook(error) if error.retryable);
            let mut error = AlienError::new(match failure {
                Failure::Hook(error) if error.code == HookError::UNCODED => ErrorData::HookFailed {
                    extension,
                    hook,
                    message: error.message,
                },
                Failure::Hook(error) => ErrorData::HookFailedWithCode {
                    extension,
                    hook,
                    code: error.code,
                    message: error.message,
                },
                Failure::Deadline => ErrorData::HookDeadlineExceeded {
                    extension,
                    hook,
                    deadline_ms: limits.deadline.as_millis() as u64,
                },
                Failure::Memory(message) => ErrorData::HookMemoryLimitExceeded {
                    extension,
                    hook,
                    limit_bytes: limits.memory_bytes,
                    message,
                },
                Failure::Output(output_bytes) => ErrorData::HookOutputLimitExceeded {
                    extension,
                    hook,
                    output_bytes,
                    limit_bytes: limits.output_bytes,
                },
                Failure::Trap(message) => ErrorData::HookTrapped {
                    extension,
                    hook,
                    message,
                },
            });
            if retryable {
                error.retryable = true;
            }
            error
        })
    }
}

/// A WASI command module loaded to run in the hooks sandbox: no network, no files, no
/// environment, every clock at 0 and a seeded random source. Unlike a hook, it gets arguments,
/// its stdout is kept, and `proc_exit` ends the run with its code.
#[derive(Clone)]
pub struct CommandModule {
    pre: InstancePre<Host>,
}

/// How a command run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandExit {
    /// `_start` returned, which is code 0, or the module called `proc_exit` with the code
    Code(i32),
    /// The module trapped, as a panic in a wasm32-wasip1 binary does
    Trapped(String),
    /// The run passed its deadline and was stopped
    DeadlineExceeded,
    /// A memory grow over the limit was refused, and the module failed because of it
    MemoryLimitExceeded(String),
}

/// What a command run wrote, and how it ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub exit: CommandExit,
    /// Up to the limit's `output_bytes`
    pub stdout: Vec<u8>,
    /// Up to 64 KiB
    pub stderr: Vec<u8>,
}

impl CommandModule {
    /// Runs `_start` in a fresh instance with `args` (the first is the program's name), within
    /// `limits`.
    ///
    /// The run blocks its thread for up to `limits.deadline`.
    pub fn run(&self, args: &[&str], limits: &Limits) -> CommandOutput {
        let mut store = Store::new(
            self.pre.module().engine(),
            Host {
                args: args.iter().map(|arg| arg.as_bytes().to_vec()).collect(),
                stdout_limit: limits.output_bytes,
                ..Host::new(limits.memory_bytes)
            },
        );
        store.limiter(|host| host);
        store.set_epoch_deadline(deadline_ticks(limits.deadline));
        let result = self
            .pre
            .instantiate(&mut store)
            .and_then(|instance| instance.get_typed_func::<(), ()>(&mut store, "_start"))
            .and_then(|start| start.call(&mut store, ()));
        // The output carries stderr, so the exit names only the trap, without the backtrace.
        let exit = match result {
            Ok(()) => CommandExit::Code(0),
            Err(error) => match error.downcast_ref::<wasi::ProcExit>() {
                Some(wasi::ProcExit(code)) => CommandExit::Code(*code),
                None if error.downcast_ref::<Trap>() == Some(&Trap::Interrupt) => {
                    CommandExit::DeadlineExceeded
                }
                None if store.data().memory_refused => {
                    CommandExit::MemoryLimitExceeded(error.root_cause().to_string())
                }
                None => CommandExit::Trapped(error.root_cause().to_string()),
            },
        };
        let host = store.into_data();
        CommandOutput {
            exit,
            stdout: host.stdout,
            stderr: host.stderr,
        }
    }
}

/// Instantiates the module in a new store with `limits`, runs `invoke`, and reads the output its
/// packed result points to.
fn run(
    pre: &InstancePre<Host>,
    limits: &Limits,
    invoke: impl FnOnce(&mut Store<Host>, &Abi) -> wasmtime::Result<i64>,
) -> std::result::Result<Vec<u8>, Failure> {
    let mut store = Store::new(pre.module().engine(), Host::new(limits.memory_bytes));
    store.limiter(|host| host);
    store.set_epoch_deadline(deadline_ticks(limits.deadline));

    let packed = Abi::instantiate(pre, &mut store)
        .and_then(|abi| invoke(&mut store, &abi).map(|packed| (abi, packed)));
    let (abi, packed) = packed.map_err(|error| Failure::classify(error, store.data()))?;

    let packed = packed as u64;
    let ptr = (packed & 0xffff_ffff) as usize;
    let len = ((packed >> 32) & 0x7fff_ffff) as usize;
    if len > limits.output_bytes {
        return Err(Failure::Output(len));
    }
    let output = abi
        .memory
        .data(&store)
        .get(ptr..ptr + len)
        .ok_or_else(|| {
            Failure::Trap(format!(
                "the module returned output at {ptr}+{len}, outside its memory"
            ))
        })?
        .to_vec();
    if packed >> 63 == 1 {
        return Err(Failure::Hook(hook_error(&output)));
    }
    Ok(output)
}

/// The most of a hook's error text an error keeps. It is saved with the deployment and shown to
/// whoever reads it, so a hook can't pass on much of its input through it.
pub const MAX_HOOK_ERROR_BYTES: usize = 1024;

/// The longest code a hook's error keeps.
const MAX_HOOK_ERROR_CODE_LEN: usize = 64;

/// The error a hook returned: the SDK writes it as a [`HookError`] in JSON, and a module that
/// writes plain text gets it as the message of an uncoded error. The message is cut to
/// [`MAX_HOOK_ERROR_BYTES`]. A code that is not a short `[A-Z0-9_]` token becomes
/// [`HookError::UNCODED`]: it is shown wherever the error is, so it can't carry text of its own.
fn hook_error(output: &[u8]) -> HookError {
    let mut error = serde_json::from_slice::<HookError>(output)
        .unwrap_or_else(|_| HookError::new(String::from_utf8_lossy(output)));
    error.message = error_text(error.message.as_bytes());
    let token = !error.code.is_empty()
        && error.code.len() <= MAX_HOOK_ERROR_CODE_LEN
        && error
            .code
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_');
    if !token {
        error.code = HookError::UNCODED.to_string();
    }
    error
}

/// A hook's error text, cut to [`MAX_HOOK_ERROR_BYTES`] on a character boundary. Control
/// characters other than a tab become spaces: the text lands in log lines, where a line break or a
/// terminal escape would start a line of its own.
fn error_text(output: &[u8]) -> String {
    let text: String = String::from_utf8_lossy(output)
        .chars()
        .map(|c| if c.is_control() && c != '\t' { ' ' } else { c })
        .collect();
    if text.len() <= MAX_HOOK_ERROR_BYTES {
        return text;
    }
    let mut end = MAX_HOOK_ERROR_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… ({} more bytes)", &text[..end], text.len() - end)
}

/// Deadlines round up to whole ticks, plus one, because the epoch lags the wall clock by up to
/// a tick while the epoch thread sleeps. A hook is stopped within a few milliseconds of its
/// deadline, either side.
fn deadline_ticks(deadline: Duration) -> u64 {
    (deadline.as_nanos().div_ceil(EPOCH_TICK.as_nanos()) as u64).saturating_add(1)
}

/// The exports the host calls, looked up with their types in one instance.
struct Abi {
    memory: Memory,
    alloc: TypedFunc<i32, i32>,
    manifest: TypedFunc<(), i64>,
    call: TypedFunc<(i32, i32, i32, i32), i64>,
}

impl Abi {
    fn instantiate(pre: &InstancePre<Host>, store: &mut Store<Host>) -> wasmtime::Result<Self> {
        let instance: Instance = pre.instantiate(&mut *store)?;
        if let Some(initialize) = instance.get_func(&mut *store, "_initialize") {
            initialize.typed::<(), ()>(&*store)?.call(&mut *store, ())?;
        }
        instance.get_typed_func::<(), ()>(&mut *store, ABI_MARKER)?;
        Ok(Self {
            memory: instance
                .get_memory(&mut *store, "memory")
                .ok_or_else(|| wasmtime::format_err!("the module exports no `memory`"))?,
            alloc: instance.get_typed_func(&mut *store, "alien_alloc")?,
            manifest: instance.get_typed_func(&mut *store, "alien_hooks_manifest")?,
            call: instance.get_typed_func(&mut *store, "alien_hooks_call")?,
        })
    }

    /// Copies `bytes` into memory the module allocates, and returns where they are.
    fn write(&self, store: &mut Store<Host>, bytes: &[u8]) -> wasmtime::Result<(i32, i32)> {
        let len = i32::try_from(bytes.len()).map_err(|_| {
            wasmtime::format_err!("{} bytes do not fit a 32-bit module", bytes.len())
        })?;
        let ptr = self.alloc.call(&mut *store, len)?;
        self.memory
            .write(&mut *store, ptr as u32 as usize, bytes)
            .map_err(|_| {
                wasmtime::format_err!("`alien_alloc` returned memory the module does not have")
            })?;
        Ok((ptr, len))
    }
}

/// Why a call into the module failed, before it is named for the caller.
enum Failure {
    /// The hook returned an error
    Hook(HookError),
    /// The epoch deadline passed
    Deadline,
    /// A memory grow over the limit was refused, and the module failed because of it
    Memory(String),
    /// The output was over the limit; its size
    Output(usize),
    /// Any other trap, with what the module wrote to stderr
    Trap(String),
}

impl Failure {
    fn classify(error: wasmtime::Error, host: &Host) -> Self {
        if error.downcast_ref::<Trap>() == Some(&Trap::Interrupt) {
            return Failure::Deadline;
        }
        let mut message = format!("{error:#}");
        if !host.stderr.is_empty() {
            message.push_str("; stderr: ");
            message.push_str(String::from_utf8_lossy(&host.stderr).trim_end());
        }
        if host.memory_refused {
            Failure::Memory(message)
        } else {
            Failure::Trap(message)
        }
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Hook(error) => write!(f, "it returned an error: {error}"),
            Failure::Deadline => write!(f, "it ran past its deadline"),
            Failure::Memory(message) => write!(f, "it ran out of memory: {message}"),
            Failure::Output(len) => write!(f, "its output of {len} bytes is over the limit"),
            Failure::Trap(message) => write!(f, "{message}"),
        }
    }
}

/// The state of one call: its memory limit, what it wrote, and its random source. A hook gets no
/// arguments and its stdout is dropped; a command gets both.
struct Host {
    memory_limit: usize,
    memory_refused: bool,
    stderr: Vec<u8>,
    stdout: Vec<u8>,
    stdout_limit: usize,
    args: Vec<Vec<u8>>,
    random: u64,
}

impl Host {
    fn new(memory_limit: usize) -> Self {
        Self {
            memory_limit,
            memory_refused: false,
            stderr: Vec::new(),
            stdout: Vec::new(),
            stdout_limit: 0,
            args: Vec::new(),
            random: wasi::RANDOM_SEED,
        }
    }
}

impl ResourceLimiter for Host {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        if desired > self.memory_limit {
            self.memory_refused = true;
            return Ok(false);
        }
        Ok(true)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(desired <= TABLE_ELEMENTS_LIMIT)
    }

    // A Rust module has one memory and one table; more would multiply what the limits allow.
    fn memories(&self) -> usize {
        1
    }

    fn tables(&self) -> usize {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hook's error lands in logs and in the deployment's error, so a line break or an escape
    /// in its message can't start a line of its own there. A tab stays.
    #[test]
    fn a_hook_error_s_message_keeps_no_control_character_but_tab() {
        let coded = hook_error(
            br#"{"code":"ZONE_LIMIT","message":"a\nINFO second\r\u001b[2Kb\tc\u0085d","retryable":false}"#,
        );
        let plain = hook_error(b"a\nINFO second\x1b[2K\tc");
        for error in [coded, plain] {
            assert!(
                !error.message.chars().any(|c| c.is_control() && c != '\t'),
                "{:?}",
                error.message
            );
            assert!(error.message.contains("second"), "{:?}", error.message);
            assert!(error.message.contains('\t'), "{:?}", error.message);
        }
    }
}
