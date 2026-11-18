//! The runtime against hand-written controller modules: what it loads, what a step may use, and
//! what a step can reach.

use alien_error::AlienError;
use alien_hooks_runtime::{
    Capabilities, ControllerModule, ErrorData, Runtime, StepFailure, StepLimits,
    MAX_HOOK_ERROR_BYTES,
};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// A controller module whose step is `step`, with `imports` before it. `alien_alloc` hands out
/// memory from 4096 up, so a step may use the bytes below for its output.
fn wasm(imports: &str, step: &str) -> Vec<u8> {
    wat::parse_str(format!(
        r#"(module {imports}
            (memory (export "memory") 2)
            (global $next (mut i32) (i32.const 4096))
            (func (export "alien_controller_abi_v1"))
            (func (export "alien_alloc") (param $len i32) (result i32)
                (local $at i32)
                (local.set $at (global.get $next))
                (global.set $next (i32.add (global.get $next) (local.get $len)))
                (local.get $at))
            (func (export "alien_controller_step") (param $ptr i32) (param $len i32) (result i64)
                {step}))"#
    ))
    .expect("the test module parses")
}

fn load(runtime: &Runtime, bytes: &[u8]) -> ControllerModule {
    runtime
        .load_controller(bytes, &sha256(bytes))
        .expect("the module loads")
}

fn refusal(bytes: &[u8]) -> String {
    let runtime = Runtime::new().unwrap();
    let error: AlienError<ErrorData> = runtime
        .load_controller(bytes, &sha256(bytes))
        .expect_err("the module is refused");
    match error.error.expect("the error carries its data") {
        ErrorData::ControllerModuleRefused { reason, .. } => reason,
        other => panic!("expected ControllerModuleRefused, got {other:?}"),
    }
}

fn limits() -> StepLimits {
    StepLimits::within(Duration::from_secs(10))
}

/// Capabilities no step in the test calls.
struct Unused;

impl Capabilities for Unused {
    fn cloud(&mut self, _: &[u8]) -> Result<Vec<u8>, String> {
        panic!("the step called the cloud")
    }
    fn http(&mut self, _: &[u8]) -> Result<Vec<u8>, String> {
        panic!("the step called a declared origin")
    }
}

fn step(module: &ControllerModule, limits: &StepLimits) -> Result<Vec<u8>, StepFailure> {
    module.step(b"{}", limits, Box::new(Unused))
}

/// Answers each call with what the test queued for it, and keeps what the module sent.
#[derive(Clone, Default)]
struct Recorded {
    cloud: Arc<Mutex<Vec<Vec<u8>>>>,
    http: Arc<Mutex<Vec<Vec<u8>>>>,
    answer: Arc<Mutex<Option<Result<Vec<u8>, String>>>>,
}

impl Recorded {
    fn answering(answer: Result<&str, &str>) -> Self {
        let recorded = Self::default();
        *recorded.answer.lock().unwrap() = Some(
            answer
                .map(|reply| reply.as_bytes().to_vec())
                .map_err(str::to_string),
        );
        recorded
    }
}

impl Capabilities for Recorded {
    fn cloud(&mut self, request: &[u8]) -> Result<Vec<u8>, String> {
        self.cloud.lock().unwrap().push(request.to_vec());
        self.answer.lock().unwrap().clone().expect("an answer")
    }
    fn http(&mut self, request: &[u8]) -> Result<Vec<u8>, String> {
        self.http.lock().unwrap().push(request.to_vec());
        self.answer.lock().unwrap().clone().expect("an answer")
    }
}

const LOOP: &str = "(loop $forever (br $forever)) (i64.const 0)";

#[test]
fn the_pinned_digest_and_the_controller_abi_are_checked() {
    let runtime = Runtime::new().unwrap();
    let bytes = wasm("", "(i64.const 0)");

    let error = runtime
        .load_controller(&bytes, &"0".repeat(64))
        .expect_err("another digest is refused");
    assert_eq!(error.code, "CONTROLLER_MODULE_DIGEST_MISMATCH");

    let hooks_module = wat::parse_str(
        r#"(module (memory (export "memory") 1) (func (export "alien_hooks_abi_v1")))"#,
    )
    .unwrap();
    let reason = refusal(&hooks_module);
    assert!(reason.contains("alien_controller_abi_v1"), "{reason}");

    // A controller module is not a hooks module either.
    let error = runtime
        .load(&bytes, &sha256(&bytes))
        .err()
        .expect("the hooks loader refuses a controller module");
    assert_eq!(error.code, "HOOKS_MODULE_REFUSED");
}

/// A module gets WASI and the two capabilities, and nothing else it asks for.
#[test]
fn a_module_importing_anything_but_wasi_and_the_two_capabilities_is_refused() {
    for import in [
        r#"(import "env" "connect" (func (param i32 i32) (result i32)))"#,
        r#"(import "alien_cloud_v1" "sign" (func (param i32 i32) (result i64)))"#,
        r#"(import "alien_cloud_v2" "send" (func (param i32 i32) (result i64)))"#,
        r#"(import "wasi:http/outgoing-handler" "handle" (func (param i32 i32) (result i32)))"#,
    ] {
        let reason = refusal(&wasm(import, "(i64.const 0)"));
        assert!(
            reason.contains("a controller module may import only"),
            "{import}: {reason}"
        );
    }
    // The capability's own name with another type is refused too.
    let reason = refusal(&wasm(
        r#"(import "alien_cloud_v1" "send" (func (param i32) (result i32)))"#,
        "(i64.const 0)",
    ));
    assert!(reason.contains("alien_cloud_v1"), "{reason}");

    let runtime = Runtime::new().unwrap();
    load(
        &runtime,
        &wasm(
            r#"(import "alien_cloud_v1" "send" (func (param i32 i32) (result i64)))
               (import "alien_http_v1" "send" (func (param i32 i32) (result i64)))
               (import "wasi_snapshot_preview1" "sock_accept" (func (param i32 i32 i32) (result i32)))"#,
            "(i64.const 0)",
        ),
    );
}

/// Two steps of one compiled module share nothing: a global and a byte the first step wrote are
/// back at their start for the second.
#[test]
fn every_step_runs_in_a_fresh_instance() {
    let runtime = Runtime::new().unwrap();
    let bytes = wasm(
        "(global $count (mut i32) (i32.const 0))",
        r#"(global.set $count (i32.add (global.get $count) (i32.const 1)))
           (i32.store8 (i32.const 0) (global.get $count))
           (i32.store8 (i32.const 1) (i32.load8_u (i32.const 64)))
           (i32.store8 (i32.const 64) (i32.const 170))
           (i64.const 0x200000000)"#,
    );
    let first = load(&runtime, &bytes);
    let second = load(&runtime, &bytes);
    for module in [&first, &second, &first] {
        assert_eq!(step(module, &limits()), Ok(vec![1, 0]));
    }
}

#[test]
fn a_step_that_loops_is_stopped_at_its_deadline() {
    let runtime = Runtime::new().unwrap();
    let module = load(&runtime, &wasm("", LOOP));
    let limits = StepLimits {
        fuel: u64::MAX,
        ..StepLimits::within(Duration::from_millis(200))
    };
    let started = Instant::now();
    assert_eq!(step(&module, &limits), Err(StepFailure::Deadline));
    let took = started.elapsed();
    // A deadline is counted in epoch ticks from the engine's epoch, which trails the clock by the
    // time since the epoch thread last ran, so the step may stop a few milliseconds early.
    assert!(
        took >= Duration::from_millis(195) && took < Duration::from_secs(5),
        "{took:?}"
    );
}

/// A step that only computes is stopped by its fuel, long before a deadline of minutes.
#[test]
fn a_step_that_loops_is_stopped_when_its_fuel_is_spent() {
    let runtime = Runtime::new().unwrap();
    let module = load(&runtime, &wasm("", LOOP));
    let limits = StepLimits {
        fuel: 10_000_000,
        ..StepLimits::within(Duration::from_secs(600))
    };
    let started = Instant::now();
    assert_eq!(step(&module, &limits), Err(StepFailure::Fuel));
    assert!(started.elapsed() < Duration::from_secs(60));
}

#[test]
fn a_step_that_needs_more_memory_than_its_limit_fails() {
    let runtime = Runtime::new().unwrap();
    let module = load(
        &runtime,
        &wasm(
            "",
            r#"(if (i32.eq (memory.grow (i32.const 2000)) (i32.const -1)) (then unreachable))
               (i64.const 0)"#,
        ),
    );
    match step(&module, &limits()) {
        Err(StepFailure::Memory(_)) => {}
        other => panic!("expected a memory failure, got {other:?}"),
    }
    // The same grow fits a limit above it.
    let roomy = StepLimits {
        memory_bytes: 256 << 20,
        ..limits()
    };
    assert_eq!(step(&module, &roomy), Ok(Vec::new()));
}

#[test]
fn a_reply_over_the_output_limit_is_refused() {
    let runtime = Runtime::new().unwrap();
    // 65537 bytes from 0, in a memory of two pages.
    let module = load(&runtime, &wasm("", "(i64.const 0x1000100000000)"));
    let limits = StepLimits {
        output_bytes: 65536,
        ..limits()
    };
    assert_eq!(step(&module, &limits), Err(StepFailure::Output(65537)));
}

/// What a module says when it can't answer, and what it writes before it traps, are kept only
/// up to the hook error limit: both are saved with the deployment.
#[test]
fn a_module_s_own_error_text_is_cut() {
    let runtime = Runtime::new().unwrap();
    // The error bit, with 65536 bytes from 0 as the message.
    let module = load(&runtime, &wasm("", "(i64.const 0x8001000000000000)"));
    match step(&module, &limits()) {
        Err(StepFailure::NoReply(message)) => {
            assert!(
                message.len() < MAX_HOOK_ERROR_BYTES + 64,
                "{}",
                message.len()
            );
            assert!(message.contains("more bytes"), "{message}");
        }
        other => panic!("expected the module's error, got {other:?}"),
    }

    let trapping = load(&runtime, &wasm("", "unreachable"));
    match step(&trapping, &limits()) {
        Err(StepFailure::Trap(message)) => {
            assert!(
                message.len() < MAX_HOOK_ERROR_BYTES + 64,
                "{}",
                message.len()
            );
            assert!(message.contains("unreachable"), "{message}");
        }
        other => panic!("expected a trap, got {other:?}"),
    }
}

/// A step sends what it wants through the import and reads the host's answer, or its refusal
/// with the error bit set; each import reaches its own capability.
#[test]
fn a_step_s_imports_reach_its_capabilities() {
    let runtime = Runtime::new().unwrap();
    let forward = |import: &str| {
        wasm(
            &format!(r#"(import "{import}" "send" (func $send (param i32 i32) (result i64)))"#),
            "(call $send (local.get $ptr) (local.get $len))",
        )
    };
    let cloud = load(&runtime, &forward("alien_cloud_v1"));
    let http = load(&runtime, &forward("alien_http_v1"));

    let recorded = Recorded::answering(Ok("the cloud's answer"));
    let reply = cloud.step(b"a cloud request", &limits(), Box::new(recorded.clone()));
    assert_eq!(reply, Ok(b"the cloud's answer".to_vec()));
    assert_eq!(
        *recorded.cloud.lock().unwrap(),
        [b"a cloud request".to_vec()]
    );
    assert!(recorded.http.lock().unwrap().is_empty());

    let recorded = Recorded::answering(Ok("an origin's answer"));
    let reply = http.step(b"an http request", &limits(), Box::new(recorded.clone()));
    assert_eq!(reply, Ok(b"an origin's answer".to_vec()));
    assert_eq!(
        *recorded.http.lock().unwrap(),
        [b"an http request".to_vec()]
    );
    assert!(recorded.cloud.lock().unwrap().is_empty());

    // The module returns the host's packed answer as its own, so a refusal's error bit shows.
    let refused = Recorded::answering(Err("refused: not a cloud endpoint"));
    let reply = cloud.step(b"elsewhere", &limits(), Box::new(refused));
    assert_eq!(
        reply,
        Err(StepFailure::NoReply(
            "refused: not a cloud endpoint".to_string()
        ))
    );
}

/// A step reads the time, how long it has run, and random bytes that differ from step to step.
/// A hook reads zeros and a fixed sequence instead, which `tests/hooks.rs` pins.
#[test]
fn a_step_reads_the_real_clock_and_a_real_random_source() {
    let runtime = Runtime::new().unwrap();
    let module = load(
        &runtime,
        &wasm(
            r#"(import "wasi_snapshot_preview1" "clock_time_get" (func $clock (param i32 i64 i32) (result i32)))
               (import "wasi_snapshot_preview1" "random_get" (func $random (param i32 i32) (result i32)))"#,
            r#"(drop (call $clock (i32.const 0) (i64.const 0) (i32.const 0)))
               (drop (call $clock (i32.const 1) (i64.const 0) (i32.const 8)))
               (drop (call $random (i32.const 16) (i32.const 16)))
               (i64.const 0x2000000000)"#,
        ),
    );
    let read = || {
        let output = step(&module, &limits()).expect("the step runs");
        let nanos = |at: usize| u64::from_le_bytes(output[at..at + 8].try_into().unwrap());
        (nanos(0), nanos(8), output[16..32].to_vec())
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    let (realtime, monotonic, random) = read();
    let minute = 60_000_000_000;
    assert!(realtime.abs_diff(now) < minute, "{realtime} vs {now}");
    assert!(monotonic < minute, "{monotonic}");
    let (_, _, random_again) = read();
    assert_ne!(random, random_again);
}
