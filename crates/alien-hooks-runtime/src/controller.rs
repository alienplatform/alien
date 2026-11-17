//! Runs an extension's controller module: one step of the step protocol per call, each in a
//! fresh instance, so nothing a step leaves in memory reaches the next one.
//!
//! The controller ABI, version 1. Besides `memory`, `_initialize` and `alien_alloc`, as a hooks
//! module has them, the module exports:
//!
//! - `alien_controller_abi_v1: () -> ()`, the version marker.
//! - `alien_controller_step: (ptr: i32, len: i32) -> i64`, which answers the step request at
//!   `ptr` with a step reply, packed as a hook's output is. Bit 63 set means the module could
//!   not answer, and the output says why.
//!
//! Besides WASI preview 1, the module may import:
//!
//! - `alien_cloud_v1::send: (ptr: i32, len: i32) -> i64`, a request to the cloud the step runs
//!   on.
//! - `alien_http_v1::send: (ptr: i32, len: i32) -> i64`, a request to an origin the extension
//!   declared.
//!
//! Each takes the request at `ptr` and returns a packed output in memory the host gets from
//! `alien_alloc`: the reply, or with bit 63 set why the host refused or could not send the
//! request. What a request and a reply hold is up to the caller's [`Capabilities`].
//!
//! A step is not held to a hook's determinism: it reads the real clocks and a real random
//! source. It gets the same sandbox otherwise: no socket, no file, no environment.

use crate::{deadline_ticks, error_text, wasi, ErrorData, Host, Result, Runtime, MAX_WASM_STACK};
use alien_error::{AlienError, Context, IntoAlienError};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};
use wasmtime::{Caller, Config, Engine, InstancePre, Linker, Memory, Module, Store, Trap};

const ABI_MARKER: &str = "alien_controller_abi_v1";

/// The import a controller module sends its cloud requests through.
pub const CLOUD_IMPORT: (&str, &str) = ("alien_cloud_v1", "send");

/// The import a controller module sends requests to its extension's declared origins through.
pub const HTTP_IMPORT: (&str, &str) = ("alien_http_v1", "send");

const ERROR_BIT: u64 = 1 << 63;

/// How many compiled controller modules a runtime keeps. A manager serves many deployments'
/// extensions over its life; past this, the module loaded longest ago is compiled again when a
/// deployment needs it.
pub const MAX_CACHED_CONTROLLERS: usize = 256;

/// How long a compiled controller module stays without a load before the next load drops it.
/// A host loads a deployment's modules every time it steps the deployment, so a module nobody
/// loaded for this long belongs to no deployment the host still steps.
pub const CONTROLLER_IDLE: Duration = Duration::from_secs(600);

/// What one step of a controller module may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepLimits {
    /// The most linear memory the instance may grow to
    pub memory_bytes: usize,
    /// The largest reply the step may return
    pub output_bytes: usize,
    /// How long the step may run, the time its host calls take included
    pub deadline: Duration,
    /// How much the step may compute, in wasmtime's fuel: about one unit per WebAssembly
    /// instruction. Time spent in a host call burns none.
    pub fuel: u64,
}

impl StepLimits {
    /// The limits of a step that must end within `deadline`.
    ///
    /// A step that sends one request through an AWS SDK and parses a short answer burns about
    /// two million units; a module that only loops spends this fuel in a fraction of a second.
    /// The deadline alone would let that module hold a core for the whole step.
    pub fn within(deadline: Duration) -> Self {
        Self {
            memory_bytes: 64 << 20,
            output_bytes: 16 << 20,
            deadline,
            fuel: 4_000_000_000,
        }
    }
}

/// What one step of a controller may call. The host builds one per step, with what that step
/// alone may reach.
///
/// A call blocks the step's thread until it answers, and the step's deadline can't interrupt
/// it there: an implementation bounds each call by the time the step has left.
pub trait Capabilities: Send {
    /// Answers a request sent through `alien_cloud_v1::send`, or says why it is refused.
    fn cloud(&mut self, request: &[u8]) -> std::result::Result<Vec<u8>, String>;
    /// Answers a request sent through `alien_http_v1::send`, or says why it is refused.
    fn http(&mut self, request: &[u8]) -> std::result::Result<Vec<u8>, String>;
}

/// The part of a call's state only a controller step has.
pub(crate) struct Step {
    capabilities: Box<dyn Capabilities>,
    pub(crate) started: Instant,
}

/// Why a step got no reply from the module. Every message is cut to
/// [`crate::MAX_HOOK_ERROR_BYTES`]: the module wrote it, and it is saved with the deployment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepFailure {
    /// The module returned an error instead of a reply
    NoReply(String),
    /// The step ran past its deadline and was stopped
    Deadline,
    /// The step used up its fuel and was stopped
    Fuel,
    /// A memory grow over the limit was refused, and the module failed because of it
    Memory(String),
    /// The reply was over the output limit; its size
    Output(usize),
    /// Any other trap, with what the module wrote to stderr
    Trap(String),
}

impl StepFailure {
    fn classify(error: wasmtime::Error, host: &Host) -> Self {
        match error.downcast_ref::<Trap>() {
            Some(Trap::Interrupt) => return StepFailure::Deadline,
            Some(Trap::OutOfFuel) => return StepFailure::Fuel,
            _ => {}
        }
        // The trap without its backtrace, which would crowd out what the module wrote: a
        // panic's message is on stderr.
        let mut message = error.root_cause().to_string();
        if !host.stderr.is_empty() {
            message.push_str("; stderr: ");
            message.push_str(String::from_utf8_lossy(&host.stderr).trim_end());
        }
        let message = error_text(message.as_bytes());
        if host.memory_refused {
            StepFailure::Memory(message)
        } else {
            StepFailure::Trap(message)
        }
    }
}

/// A loaded controller module. Cloning it is cheap, and every clone shares the compiled module.
#[derive(Clone)]
pub struct ControllerModule {
    sha256: String,
    pre: InstancePre<Host>,
}

impl std::fmt::Debug for ControllerModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControllerModule")
            .field("sha256", &self.sha256)
            .finish_non_exhaustive()
    }
}

struct Cached {
    module: ControllerModule,
    loaded: Instant,
}

/// The engine controller modules compile against, and the compiled modules, least recently
/// loaded first.
pub(crate) struct Controllers {
    engine: Engine,
    cache: Mutex<VecDeque<Cached>>,
}

impl Controllers {
    /// Controller modules get an engine of their own: counting fuel slows every module compiled
    /// for the engine, and a hook has a deadline short enough to need none.
    fn start() -> Result<Self> {
        let mut config = Config::new();
        config
            .epoch_interruption(true)
            .consume_fuel(true)
            .max_wasm_stack(MAX_WASM_STACK);
        let engine = Engine::new(&config).map_err(|error| {
            AlienError::new(ErrorData::RuntimeStartFailed {
                message: format!("{error:#}"),
            })
        })?;
        let epoch = engine.weak();
        thread::Builder::new()
            .name("alien-controllers-epoch".to_string())
            .spawn(move || crate::advance_epoch(epoch))
            .into_alien_error()
            .context(ErrorData::RuntimeStartFailed {
                message: "the controllers' epoch thread did not start".to_string(),
            })?;
        Ok(Self {
            engine,
            cache: Mutex::default(),
        })
    }

    fn cache(&self) -> MutexGuard<'_, VecDeque<Cached>> {
        // The queue holds no invariant a panic could break halfway.
        self.cache.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn compile(&self, wasm: &[u8]) -> std::result::Result<InstancePre<Host>, String> {
        let module = Module::new(&self.engine, wasm)
            .map_err(|error| format!("it is not a valid WebAssembly module: {error:#}"))?;
        if module.get_export(ABI_MARKER).is_none() {
            return Err(format!(
                "it is not an Alien controller module, or it was built for another controller ABI: it has no `{ABI_MARKER}` export"
            ));
        }
        if let Some(import) = module.imports().find(|import| {
            let name = (import.module(), import.name());
            import.module() != wasi::WASI_MODULE && name != CLOUD_IMPORT && name != HTTP_IMPORT
        }) {
            return Err(format!(
                "it imports `{}::{}`, and a controller module may import only from {}, `{}::{}` and `{}::{}`",
                import.module(),
                import.name(),
                wasi::WASI_MODULE,
                CLOUD_IMPORT.0,
                CLOUD_IMPORT.1,
                HTTP_IMPORT.0,
                HTTP_IMPORT.1,
            ));
        }
        let mut linker = wasi::wasi_linker(&self.engine, &module)?;
        define_capabilities(&mut linker).map_err(|error| format!("{error:#}"))?;
        linker
            .instantiate_pre(&module)
            .map_err(|error| format!("{error:#}"))
    }
}

/// The module with `sha256`, moved to the most recently loaded end.
fn cached(cache: &mut VecDeque<Cached>, sha256: &str) -> Option<ControllerModule> {
    let at = cache
        .iter()
        .position(|cached| cached.module.sha256 == sha256)?;
    let mut hit = cache.remove(at)?;
    hit.loaded = Instant::now();
    let module = hit.module.clone();
    cache.push_back(hit);
    Some(module)
}

fn evict_idle(cache: &mut VecDeque<Cached>, idle: Duration) {
    while cache
        .front()
        .is_some_and(|oldest| oldest.loaded.elapsed() >= idle)
    {
        cache.pop_front();
    }
}

impl Runtime {
    /// Loads a controller module: checks that its sha256 is `expected_sha256`, then compiles it
    /// and checks its ABI and its imports, unless a module with that digest is already loaded.
    ///
    /// The runtime keeps the [`MAX_CACHED_CONTROLLERS`] modules loaded last, and drops one no
    /// load asked for in [`CONTROLLER_IDLE`]. A module a caller still holds stays compiled
    /// until the caller drops it, so a host that keeps a module only while it steps a
    /// deployment keeps its memory within the cache's.
    pub fn load_controller(&self, wasm: &[u8], expected_sha256: &str) -> Result<ControllerModule> {
        let sha256 = hex::encode(Sha256::digest(wasm));
        if !sha256.eq_ignore_ascii_case(expected_sha256) {
            return Err(AlienError::new(ErrorData::ControllerModuleDigestMismatch {
                expected_sha256: expected_sha256.to_string(),
                actual_sha256: sha256,
            }));
        }
        let controllers = self.controllers()?;
        if let Some(module) = cached(&mut controllers.cache(), &sha256) {
            return Ok(module);
        }
        let refused = |reason: String| {
            AlienError::new(ErrorData::ControllerModuleRefused {
                sha256: sha256.clone(),
                reason,
            })
        };
        if wasm.len() > crate::MAX_MODULE_BYTES {
            return Err(refused(format!(
                "it is {} bytes, more than the {} a controller module may take",
                wasm.len(),
                crate::MAX_MODULE_BYTES
            )));
        }
        let pre = controllers.compile(wasm).map_err(refused)?;
        let mut cache = controllers.cache();
        // Two threads that load the same new module both compile it; the first one stored wins.
        if let Some(module) = cached(&mut cache, &sha256) {
            return Ok(module);
        }
        evict_idle(&mut cache, CONTROLLER_IDLE);
        if cache.len() >= MAX_CACHED_CONTROLLERS {
            cache.pop_front();
        }
        let module = ControllerModule { sha256, pre };
        cache.push_back(Cached {
            module: module.clone(),
            loaded: Instant::now(),
        });
        Ok(module)
    }

    /// Drops the compiled controller modules no load asked for in `idle`, and returns how many
    /// stay. [`Self::load_controller`] does this with [`CONTROLLER_IDLE`] whenever it compiles
    /// a module.
    pub fn evict_controllers_idle_for(&self, idle: Duration) -> usize {
        let Some(controllers) = self.controllers.get() else {
            return 0;
        };
        let mut cache = controllers.cache();
        evict_idle(&mut cache, idle);
        cache.len()
    }

    /// Started on the first load, so a process that runs only hooks starts one engine.
    fn controllers(&self) -> Result<&Controllers> {
        if let Some(controllers) = self.controllers.get() {
            return Ok(controllers);
        }
        let controllers = Controllers::start()?;
        Ok(self.controllers.get_or_init(|| controllers))
    }
}

impl ControllerModule {
    /// The module's sha256, in lowercase hex.
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// Runs one step on `input`, the step request, in a fresh instance whose imports call
    /// `capabilities`, and returns the module's reply.
    ///
    /// The call blocks its thread for up to `limits.deadline`. From async code, call it through
    /// `tokio::task::spawn_blocking`.
    pub fn step(
        &self,
        input: &[u8],
        limits: &StepLimits,
        capabilities: Box<dyn Capabilities>,
    ) -> std::result::Result<Vec<u8>, StepFailure> {
        let mut store = Store::new(
            self.pre.module().engine(),
            Host {
                step: Some(Step {
                    capabilities,
                    started: Instant::now(),
                }),
                ..Host::new(limits.memory_bytes)
            },
        );
        store.limiter(|host| host);
        store.set_epoch_deadline(deadline_ticks(limits.deadline));
        let stepped = store
            .set_fuel(limits.fuel)
            .and_then(|()| self.call(&mut store, input));
        let (memory, packed) =
            stepped.map_err(|error| StepFailure::classify(error, store.data()))?;

        let packed = packed as u64;
        let ptr = (packed & 0xffff_ffff) as usize;
        let len = ((packed >> 32) & 0x7fff_ffff) as usize;
        if len > limits.output_bytes {
            return Err(StepFailure::Output(len));
        }
        let output = memory.data(&store).get(ptr..ptr + len).ok_or_else(|| {
            StepFailure::Trap(format!(
                "the module returned output at {ptr}+{len}, outside its memory"
            ))
        })?;
        if packed & ERROR_BIT != 0 {
            return Err(StepFailure::NoReply(error_text(output)));
        }
        Ok(output.to_vec())
    }

    fn call(&self, store: &mut Store<Host>, input: &[u8]) -> wasmtime::Result<(Memory, i64)> {
        let instance = self.pre.instantiate(&mut *store)?;
        if let Some(initialize) = instance.get_func(&mut *store, "_initialize") {
            initialize.typed::<(), ()>(&*store)?.call(&mut *store, ())?;
        }
        let memory = instance
            .get_memory(&mut *store, "memory")
            .ok_or_else(|| wasmtime::format_err!("the module exports no `memory`"))?;
        let alloc = instance.get_typed_func::<i32, i32>(&mut *store, "alien_alloc")?;
        let step =
            instance.get_typed_func::<(i32, i32), i64>(&mut *store, "alien_controller_step")?;
        let len = i32::try_from(input.len()).map_err(|_| {
            wasmtime::format_err!("{} bytes do not fit a 32-bit module", input.len())
        })?;
        let ptr = alloc.call(&mut *store, len)?;
        memory
            .write(&mut *store, ptr as u32 as usize, input)
            .map_err(|_| {
                wasmtime::format_err!("`alien_alloc` returned memory the module does not have")
            })?;
        Ok((memory, step.call(&mut *store, (ptr, len))?))
    }
}

fn define_capabilities(linker: &mut Linker<Host>) -> wasmtime::Result<()> {
    linker.func_wrap(
        CLOUD_IMPORT.0,
        CLOUD_IMPORT.1,
        |caller: Caller<'_, Host>, ptr: i32, len: i32| {
            send(caller, ptr, len, |capabilities, request| {
                capabilities.cloud(request)
            })
        },
    )?;
    linker.func_wrap(
        HTTP_IMPORT.0,
        HTTP_IMPORT.1,
        |caller: Caller<'_, Host>, ptr: i32, len: i32| {
            send(caller, ptr, len, |capabilities, request| {
                capabilities.http(request)
            })
        },
    )?;
    Ok(())
}

/// Reads the request at `ptr`, answers it with `answer`, and writes the reply into memory the
/// module allocates for it.
fn send(
    mut caller: Caller<'_, Host>,
    ptr: i32,
    len: i32,
    answer: impl FnOnce(&mut dyn Capabilities, &[u8]) -> std::result::Result<Vec<u8>, String>,
) -> wasmtime::Result<i64> {
    let (memory, host) = wasi::memory(&mut caller)?;
    let request = wasi::slice(memory, ptr, len as u32 as usize)?.to_vec();
    let step = host
        .step
        .as_mut()
        .ok_or_else(|| wasmtime::format_err!("the instance runs no controller step"))?;
    let (reply, refused) = match answer(step.capabilities.as_mut(), &request) {
        Ok(reply) => (reply, false),
        Err(reason) => (reason.into_bytes(), true),
    };
    let reply_len = i32::try_from(reply.len()).map_err(|_| {
        wasmtime::format_err!(
            "a reply of {} bytes does not fit a 32-bit module",
            reply.len()
        )
    })?;
    let alloc = caller
        .get_export("alien_alloc")
        .and_then(|export| export.into_func())
        .ok_or_else(|| wasmtime::format_err!("the module exports no `alien_alloc`"))?
        .typed::<i32, i32>(&caller)?;
    let at = alloc.call(&mut caller, reply_len)?;
    let (memory, _) = wasi::memory(&mut caller)?;
    wasi::slice(memory, at, reply.len())?.copy_from_slice(&reply);
    let packed = at as u32 as u64 | ((reply.len() as u64) << 32);
    Ok((if refused { packed | ERROR_BIT } else { packed }) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A controller module that answers every step with nothing, made distinct by `tag`.
    fn module(tag: usize) -> (Vec<u8>, String) {
        let wasm = wat::parse_str(format!(
            r#"(module
                (memory (export "memory") 1)
                (data (i32.const 0) "{tag}")
                (func (export "alien_controller_abi_v1"))
                (func (export "alien_alloc") (param i32) (result i32) i32.const 1024)
                (func (export "alien_controller_step") (param i32 i32) (result i64) i64.const 0))"#
        ))
        .unwrap();
        let sha256 = hex::encode(Sha256::digest(&wasm));
        (wasm, sha256)
    }

    fn digests(runtime: &Runtime) -> Vec<String> {
        let controllers = runtime.controllers().unwrap();
        let cache = controllers.cache();
        cache
            .iter()
            .map(|cached| cached.module.sha256.clone())
            .collect()
    }

    /// The cache is bounded by count, and the module that goes is the one loaded longest ago,
    /// not the one compiled first.
    #[test]
    fn the_cache_keeps_the_modules_loaded_last() {
        let runtime = Runtime::new().unwrap();
        let (first, first_sha256) = module(0);
        runtime.load_controller(&first, &first_sha256).unwrap();
        let (second, second_sha256) = module(1);
        runtime.load_controller(&second, &second_sha256).unwrap();
        for tag in 2..MAX_CACHED_CONTROLLERS {
            let (wasm, sha256) = module(tag);
            runtime.load_controller(&wasm, &sha256).unwrap();
        }
        assert_eq!(digests(&runtime).len(), MAX_CACHED_CONTROLLERS);

        // A load of a module already compiled makes it the most recently loaded.
        runtime.load_controller(&first, &first_sha256).unwrap();
        let (extra, extra_sha256) = module(MAX_CACHED_CONTROLLERS);
        runtime.load_controller(&extra, &extra_sha256).unwrap();

        let kept = digests(&runtime);
        assert_eq!(kept.len(), MAX_CACHED_CONTROLLERS);
        assert!(kept.contains(&first_sha256));
        assert!(kept.contains(&extra_sha256));
        assert!(!kept.contains(&second_sha256));
    }

    /// A module nobody loaded for the idle time is dropped, one loaded since stays, and a
    /// module a caller still holds keeps running after the cache dropped it.
    #[test]
    fn a_module_nobody_loaded_lately_is_dropped() {
        let runtime = Runtime::new().unwrap();
        let (idle, idle_sha256) = module(0);
        let held = runtime.load_controller(&idle, &idle_sha256).unwrap();
        thread::sleep(Duration::from_millis(60));
        let (fresh, fresh_sha256) = module(1);
        runtime.load_controller(&fresh, &fresh_sha256).unwrap();

        assert_eq!(
            runtime.evict_controllers_idle_for(Duration::from_millis(50)),
            1
        );
        assert_eq!(digests(&runtime), vec![fresh_sha256]);

        struct Unused;
        impl Capabilities for Unused {
            fn cloud(&mut self, _: &[u8]) -> std::result::Result<Vec<u8>, String> {
                Err("unused".to_string())
            }
            fn http(&mut self, _: &[u8]) -> std::result::Result<Vec<u8>, String> {
                Err("unused".to_string())
            }
        }
        let limits = StepLimits::within(Duration::from_secs(5));
        assert_eq!(held.step(b"{}", &limits, Box::new(Unused)), Ok(Vec::new()));
    }
}
