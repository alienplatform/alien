//! The runtime against a real hooks module: `tests/guest`, built for wasm32-wasip1, and a few
//! hand-written modules that break the ABI.
//!
//! Set `HOOKS_RUNTIME_TEST_OUT` to a directory to keep the outputs the determinism tests compare,
//! so two runs, or two machines, can be compared byte for byte.

use alien_error::AlienError;
use alien_hooks_runtime::{CommandExit, ErrorData, HookKind, HookModule, Limits, Runtime};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

/// Builds the guest once per test process. Test processes share the target directory, and each
/// holds a lock on it from the build to the read, since a build that finds the module fresh can
/// still replace the file another process is about to read.
fn guest() -> &'static [u8] {
    static GUEST: OnceLock<Vec<u8>> = OnceLock::new();
    GUEST.get_or_init(|| {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/guest/Cargo.toml");
        let target_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("hooks-test-guest");
        std::fs::create_dir_all(&target_dir).expect("the target directory can be created");
        let lock = File::create(target_dir.join("test.lock")).expect("the lock file can be created");
        lock.lock().expect("the lock is taken");
        let output = Command::new(env!("CARGO"))
            .args(["build", "--release", "--locked", "--target", "wasm32-wasip1"])
            .arg("--manifest-path")
            .arg(&manifest)
            .arg("--target-dir")
            .arg(&target_dir)
            // Flags meant for the host build (a linker, a target) must not reach this one.
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("CARGO_BUILD_TARGET")
            .env_remove("CARGO_TARGET_DIR")
            .output()
            .expect("cargo runs");
        assert!(
            output.status.success(),
            "building the test guest for wasm32-wasip1 failed; `rustup target add wasm32-wasip1` installs the target:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let release = target_dir.join("wasm32-wasip1/release");
        let command = std::fs::read(release.join("command.wasm")).expect("the command was built");
        COMMAND.set(command).expect("the guest builds once");
        std::fs::read(release.join("hooks_test_guest.wasm")).expect("the guest module was built")
    })
}

static COMMAND: OnceLock<Vec<u8>> = OnceLock::new();

/// The guest's WASI command, `src/bin/command.rs`.
fn command() -> &'static [u8] {
    guest();
    COMMAND.get().expect("the guest build set it")
}

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn load_guest(runtime: &Runtime) -> HookModule {
    runtime
        .load(guest(), &sha256(guest()))
        .expect("the guest loads")
}

fn error_data(error: AlienError<ErrorData>) -> ErrorData {
    error.error.expect("the error carries its data")
}

fn save(name: &str, bytes: &[u8]) {
    if let Ok(dir) = std::env::var("HOOKS_RUNTIME_TEST_OUT") {
        std::fs::create_dir_all(&dir).expect("the output directory can be created");
        std::fs::write(Path::new(&dir).join(name), bytes).expect("the output can be written");
    }
}

fn with_deadline(deadline: Duration) -> Limits {
    Limits {
        deadline,
        ..Limits::MODIFIER_OR_EMITTER
    }
}

/// A module with the given exports and imports around a manifest in its data segment.
fn module(exports: &str, imports: &str, manifest: &str) -> Vec<u8> {
    let packed = 16u64 | ((manifest.len() as u64) << 32);
    let manifest = manifest.replace('"', "\\\"");
    wat::parse_str(format!(
        r#"(module {imports}
            (memory (export "memory") 1)
            (data (i32.const 16) "{manifest}")
            (func (export "alien_alloc") (param i32) (result i32) i32.const 1024)
            (func (export "alien_hooks_manifest") (result i64) i64.const {packed})
            {exports})"#
    ))
    .expect("the test module parses")
}

const MARKER_AND_CALL: &str = r#"(func (export "alien_hooks_abi_v1"))
    (func (export "alien_hooks_call") (param i32 i32 i32 i32) (result i64) i64.const 0)"#;
const EMPTY_MANIFEST: &str = r#"{"extension":"handmade","hooks":[]}"#;

fn refusal(wasm: &[u8]) -> String {
    let runtime = Runtime::new().unwrap();
    match error_data(
        runtime
            .load(wasm, &sha256(wasm))
            .err()
            .expect("the module is refused"),
    ) {
        ErrorData::ModuleRefused { reason, .. } => reason,
        other => panic!("expected ModuleRefused, got {other:?}"),
    }
}

#[test]
fn loads_the_module_and_reads_its_manifest() {
    let runtime = Runtime::new().unwrap();
    let started = Instant::now();
    let module = load_guest(&runtime);
    let cold = started.elapsed();
    let started = Instant::now();
    let again = load_guest(&runtime);
    let cached = started.elapsed();
    println!(
        "guest: {} bytes, cold load {cold:?}, cached load {cached:?}",
        guest().len()
    );

    assert_eq!(module.manifest.extension, "test-guest");
    let names: Vec<&str> = module
        .manifest
        .hooks
        .iter()
        .map(|h| h.name.as_str())
        .collect();
    assert_eq!(names[..3], ["echo", "fail", "spin"]);
    assert_eq!(names.len(), 18);
    let interceptor = module.manifest.hooks.last().unwrap();
    assert_eq!(interceptor.kind, HookKind::AwsInterceptor);
    assert_eq!(interceptor.routing["matches"]["service"], "rds");
    assert_eq!(again.manifest, module.manifest);
    save(
        "manifest.json",
        &serde_json::to_vec(&module.manifest).unwrap(),
    );
}

#[test]
fn refuses_a_module_whose_sha256_is_not_the_pinned_one() {
    let runtime = Runtime::new().unwrap();
    let pinned = "0".repeat(64);
    let error = error_data(runtime.load(guest(), &pinned).err().unwrap());
    assert!(
        matches!(&error, ErrorData::ModuleDigestMismatch { expected_sha256, actual_sha256 }
            if *expected_sha256 == pinned && *actual_sha256 == sha256(guest())),
        "{error:?}"
    );

    // A module already compiled under its real digest is still refused under another one.
    load_guest(&runtime);
    let error = error_data(runtime.load(guest(), &pinned).err().unwrap());
    assert!(
        matches!(error, ErrorData::ModuleDigestMismatch { .. }),
        "{error:?}"
    );
}

/// A module larger than a bundle object may be is refused before it is compiled.
#[test]
fn refuses_a_module_over_the_size_cap() {
    let oversized = vec![0u8; alien_hooks_runtime::MAX_MODULE_BYTES + 1];
    let error = error_data(
        Runtime::new()
            .unwrap()
            .load(&oversized, &sha256(&oversized))
            .err()
            .unwrap(),
    );
    assert!(
        matches!(&error, ErrorData::ModuleRefused { reason, .. } if reason.contains("more than")),
        "{error:?}"
    );
}

#[test]
fn refuses_a_module_without_the_abi_marker() {
    let call_only =
        r#"(func (export "alien_hooks_call") (param i32 i32 i32 i32) (result i64) i64.const 0)"#;
    let reason = refusal(&module(call_only, "", EMPTY_MANIFEST));
    assert!(
        reason.contains("no `alien_hooks_abi_v1` export"),
        "{reason}"
    );
}

#[test]
fn refuses_an_import_outside_wasi_preview_1() {
    let reason = refusal(&module(
        MARKER_AND_CALL,
        r#"(import "env" "fetch" (func))"#,
        EMPTY_MANIFEST,
    ));
    assert!(reason.contains("imports `env::fetch`"), "{reason}");
}

#[test]
fn refuses_a_module_whose_exports_break_the_abi() {
    let marker_only = r#"(func (export "alien_hooks_abi_v1"))"#;
    let reason = refusal(&module(marker_only, "", EMPTY_MANIFEST));
    assert!(reason.contains("alien_hooks_call"), "{reason}");

    let call_with_wrong_type = r#"(func (export "alien_hooks_abi_v1"))
        (func (export "alien_hooks_call") (param i32 i32) (result i64) i64.const 0)"#;
    let reason = refusal(&module(call_with_wrong_type, "", EMPTY_MANIFEST));
    assert!(reason.contains("alien_hooks_call"), "{reason}");
}

#[test]
fn refuses_a_manifest_with_a_hook_kind_no_host_runs() {
    let manifest = r#"{"extension":"handmade","hooks":[{"name":"x","kind":"shellCommand"}]}"#;
    let reason = refusal(&module(MARKER_AND_CALL, "", manifest));
    assert!(
        reason.contains("its manifest is not valid") && reason.contains("shellCommand"),
        "{reason}"
    );
}

#[test]
fn calls_a_hook_by_name() {
    let runtime = Runtime::new().unwrap();
    let module = load_guest(&runtime);
    let limits = Limits::INTERCEPTOR_OR_IMPORTER;

    assert_eq!(module.call("echo", b"hello", &limits).unwrap(), b"hello");
    assert_eq!(module.call("echo", b"", &limits).unwrap(), b"");

    let error = module.call("fail", b"input", &limits).unwrap_err();
    assert_eq!(
        error.message,
        "Hook 'fail' of extension 'test-guest' failed: refused 5 bytes"
    );
    let error = error_data(error);
    assert!(
        matches!(&error, ErrorData::HookFailed { extension, hook, message }
            if extension == "test-guest" && hook == "fail" && message == "refused 5 bytes"),
        "{error:?}"
    );

    // An error the SDK writes keeps its code and says whether another try may pass.
    let coded = module.call("fail-coded", b"", &limits).unwrap_err();
    assert!(coded.retryable, "{coded:?}");
    assert_eq!(coded.code, "HOOK_FAILED");
    assert!(
        matches!(error_data(coded), ErrorData::HookFailedWithCode { code, message, .. }
            if code == "ZONE_LIMIT" && message == "too many zones"),
    );

    // A code is shown wherever the error is, so one that is not a short token is dropped.
    let error = error_data(module.call("fail-text-code", b"", &limits).unwrap_err());
    assert!(
        matches!(&error, ErrorData::HookFailed { message, .. } if message == "m"),
        "{error:?}"
    );

    let error = error_data(module.call("no-such-hook", b"", &limits).unwrap_err());
    assert!(
        matches!(&error, ErrorData::HookFailed { message, .. } if message.contains("no hook named")),
        "{error:?}"
    );

    // The error is saved with the deployment, so a hook can't pass on much of its input in it.
    let long_name = "x".repeat(10_000);
    let error = error_data(module.call(&long_name, b"", &limits).unwrap_err());
    let ErrorData::HookFailed { message, .. } = &error else {
        panic!("{error:?}")
    };
    assert!(
        message.len() < alien_hooks_runtime::MAX_HOOK_ERROR_BYTES + 64,
        "{}",
        message.len()
    );
    assert!(message.ends_with("more bytes)"), "{message}");
}

#[test]
fn the_same_input_gives_the_same_bytes() {
    let first = load_guest(&Runtime::new().unwrap());
    let second = load_guest(&Runtime::new().unwrap());
    let limits = Limits::MODIFIER_OR_EMITTER;
    for hook in ["hashmap-order", "random", "clock"] {
        let output = first.call(hook, b"", &limits).unwrap();
        for module in [&first, &second] {
            for _ in 0..10 {
                assert_eq!(module.call(hook, b"", &limits).unwrap(), output, "{hook}");
            }
        }
        save(&format!("{hook}.out"), &output);
    }
    let random = first.call("random", b"", &limits).unwrap();
    assert!(
        random.iter().any(|byte| *byte != 0),
        "the random source returns bytes"
    );
    let clock = first.call("clock", b"", &limits).unwrap();
    assert_eq!(
        String::from_utf8(clock).unwrap(),
        "wallNanos: Ok(0)\nmonotonicElapsedNanos: 0\n"
    );
}

#[test]
fn every_call_gets_a_fresh_instance() {
    let module = load_guest(&Runtime::new().unwrap());
    for _ in 0..3 {
        assert_eq!(
            module
                .call("counter", b"", &Limits::INTERCEPTOR_OR_IMPORTER)
                .unwrap(),
            b"1"
        );
    }
}

#[test]
fn a_hook_that_spins_stops_at_its_deadline() {
    let runtime = Runtime::new().unwrap();
    let module = Arc::new(load_guest(&runtime));
    let limits = Limits::INTERCEPTOR_OR_IMPORTER;

    let started = Instant::now();
    let error = module.call("spin", b"", &limits).unwrap_err();
    let elapsed = started.elapsed();
    assert!(
        !error.retryable,
        "a hook gives the same answer to the same input"
    );
    let error = error_data(error);
    println!("spin with a 100 ms deadline stopped after {elapsed:?}");
    assert!(
        matches!(&error, ErrorData::HookDeadlineExceeded { hook, deadline_ms: 100, .. } if hook == "spin"),
        "{error:?}"
    );
    assert!(
        elapsed >= limits.deadline - Duration::from_millis(5),
        "stopped early, after {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(1),
        "stopped late, after {elapsed:?}"
    );

    let concurrent = with_deadline(Duration::from_millis(200));
    let started = Instant::now();
    let spins: Vec<_> = (0..8)
        .map(|_| {
            let module = module.clone();
            std::thread::spawn(move || module.call("spin", b"", &concurrent))
        })
        .collect();
    for spin in spins {
        let error = error_data(spin.join().unwrap().unwrap_err());
        assert!(
            matches!(error, ErrorData::HookDeadlineExceeded { .. }),
            "{error:?}"
        );
    }
    println!(
        "8 concurrent spins with a 200 ms deadline all stopped after {:?}",
        started.elapsed()
    );
    assert_eq!(
        module.call("echo", b"still running", &limits).unwrap(),
        b"still running"
    );
}

#[test]
fn a_hook_that_allocates_without_end_stops_at_its_memory_limit() {
    let module = load_guest(&Runtime::new().unwrap());
    let limits = Limits {
        memory_bytes: 16 << 20,
        ..with_deadline(Duration::from_secs(5))
    };
    let error = error_data(module.call("allocate-forever", b"", &limits).unwrap_err());
    assert!(
        matches!(&error, ErrorData::HookMemoryLimitExceeded { limit_bytes, message, .. }
            if *limit_bytes == 16 << 20 && message.contains("memory allocation")),
        "{error:?}"
    );
    assert_eq!(module.call("echo", b"ok", &limits).unwrap(), b"ok");

    // A module that grows its memory a page at a time until `memory.grow` fails, then returns
    // `memory.size`, shows where the limit stops it.
    let grow_until_refused = r#"(func (export "alien_hooks_abi_v1"))
        (func (export "alien_hooks_call") (param i32 i32 i32 i32) (result i64)
            (block $refused
                (loop $grow
                    (br_if $refused (i32.eq (memory.grow (i32.const 1)) (i32.const -1)))
                    (br $grow)))
            (i32.store (i32.const 0) (memory.size))
            (i64.const 17179869184))"#;
    let grow = crate::module(
        grow_until_refused,
        "",
        r#"{"extension":"handmade","hooks":[{"name":"grow","kind":"cloudformationModifier"}]}"#,
    );
    let grow = Runtime::new().unwrap().load(&grow, &sha256(&grow)).unwrap();
    let pages = grow.call("grow", b"", &limits).unwrap();
    assert_eq!(
        u32::from_le_bytes(pages.try_into().unwrap()) as usize,
        limits.memory_bytes / 65536
    );
}

#[test]
fn an_output_over_the_limit_is_refused() {
    let module = load_guest(&Runtime::new().unwrap());
    let limits = Limits {
        output_bytes: 1 << 20,
        ..Limits::MODIFIER_OR_EMITTER
    };
    let at_limit = module.call("output-of-size", b"1048576", &limits).unwrap();
    assert_eq!(at_limit.len(), 1 << 20);
    let error = error_data(
        module
            .call("output-of-size", b"1048577", &limits)
            .unwrap_err(),
    );
    assert!(
        matches!(
            error,
            ErrorData::HookOutputLimitExceeded {
                output_bytes: 1048577,
                limit_bytes: 1048576,
                ..
            }
        ),
        "{error:?}"
    );
}

#[test]
fn a_panic_returns_its_message() {
    let module = load_guest(&Runtime::new().unwrap());
    let error = error_data(
        module
            .call("panic", b"", &Limits::MODIFIER_OR_EMITTER)
            .unwrap_err(),
    );
    assert!(
        matches!(&error, ErrorData::HookTrapped { message, .. } if message.contains("the hook panicked on purpose")),
        "{error:?}"
    );
}

#[test]
fn deep_recursion_and_exit_trap_and_the_host_keeps_running() {
    let module = load_guest(&Runtime::new().unwrap());
    let limits = Limits::MODIFIER_OR_EMITTER;
    let error = error_data(module.call("recurse", b"", &limits).unwrap_err());
    assert!(matches!(error, ErrorData::HookTrapped { .. }), "{error:?}");
    let error = error_data(module.call("exit", b"", &limits).unwrap_err());
    assert!(
        matches!(&error, ErrorData::HookTrapped { message, .. } if message.contains("proc_exit(3)")),
        "{error:?}"
    );
    assert_eq!(module.call("echo", b"ok", &limits).unwrap(), b"ok");
}

#[test]
fn files_environment_and_sockets_are_closed() {
    const SECRET: &str = "host-secret-the-hook-must-not-see";
    std::env::set_var("HOOKS_RUNTIME_TEST_SECRET", SECRET);
    let module = load_guest(&Runtime::new().unwrap());
    for hook in ["open-file", "read-env", "socket"] {
        let output = String::from_utf8(
            module
                .call(hook, b"", &Limits::MODIFIER_OR_EMITTER)
                .unwrap(),
        )
        .unwrap();
        println!("{hook}:\n{output}");
        assert!(
            !output.contains(SECRET),
            "{hook} saw the host's environment"
        );
        for line in output.lines() {
            let (key, value) = line.split_once(": ").unwrap();
            if key == "vars" || key == "args" {
                assert_eq!(value, "0", "{hook} {key}");
            } else {
                assert!(value.starts_with("Err("), "{hook} {key} succeeded: {value}");
            }
        }
    }
}

#[test]
fn a_hook_that_prints_runs_inside_tokio() {
    let module = load_guest(&Runtime::new().unwrap());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let direct =
        runtime.block_on(async { module.call("print", b"x", &Limits::INTERCEPTOR_OR_IMPORTER) });
    assert_eq!(direct.unwrap(), b"x");
    let blocking = runtime.block_on(async move {
        tokio::task::spawn_blocking(move || {
            module.call("print", b"y", &Limits::INTERCEPTOR_OR_IMPORTER)
        })
        .await
    });
    assert_eq!(blocking.unwrap().unwrap(), b"y");
}

#[test]
fn the_limits_are_the_documented_ones() {
    assert_eq!(
        Limits::MODIFIER_OR_EMITTER,
        Limits {
            memory_bytes: 64 << 20,
            output_bytes: 16 << 20,
            deadline: Duration::from_secs(2)
        }
    );
    assert_eq!(
        Limits::INTERCEPTOR_OR_IMPORTER,
        Limits {
            memory_bytes: 64 << 20,
            output_bytes: 16 << 20,
            deadline: Duration::from_millis(100)
        }
    );
}

#[test]
fn a_command_gets_its_arguments_and_returns_its_stdout_and_exit_code() {
    let command = Runtime::new().unwrap().load_command(command()).unwrap();
    let output = command.run(&["command", "exit", "--flag"], &Limits::TEST_RUN);
    assert_eq!(output.exit, CommandExit::Code(7));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("args: command exit --flag"), "{stdout}");
    // The same closed sandbox as a hook: no files and no environment.
    assert!(stdout.contains("readPasswd: Err("), "{stdout}");
    assert!(stdout.contains("vars: 0"), "{stdout}");

    let output = command.run(&["command"], &Limits::TEST_RUN);
    assert_eq!(output.exit, CommandExit::Code(0));
}

#[test]
fn a_command_that_panics_traps_with_its_message() {
    let command = Runtime::new().unwrap().load_command(command()).unwrap();
    let output = command.run(&["command", "panic"], &Limits::TEST_RUN);
    assert!(
        matches!(output.exit, CommandExit::Trapped(_)),
        "{:?}",
        output.exit
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("the command was asked to panic"),
        "{stderr}"
    );
}

#[test]
fn a_command_stops_at_its_deadline() {
    let command = Runtime::new().unwrap().load_command(command()).unwrap();
    let started = Instant::now();
    let output = command.run(
        &["command", "spin"],
        &Limits {
            deadline: Duration::from_millis(200),
            ..Limits::TEST_RUN
        },
    );
    assert_eq!(output.exit, CommandExit::DeadlineExceeded);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_command_keeps_stdout_up_to_its_limit() {
    let command = Runtime::new().unwrap().load_command(command()).unwrap();
    let output = command.run(
        &["command"],
        &Limits {
            output_bytes: 10,
            ..Limits::TEST_RUN
        },
    );
    assert_eq!(output.stdout, b"args: comm");
}

#[test]
fn refuses_a_command_without_start() {
    let error = Runtime::new().unwrap().load_command(guest()).err().unwrap();
    match error_data(error) {
        ErrorData::ModuleRefused { reason, .. } => assert!(reason.contains("`_start`"), "{reason}"),
        other => panic!("expected ModuleRefused, got {other:?}"),
    }
}
