//! alien-launcher — supervisor for the alien-operator OS-service packaging.
//!
//! Sits between the OS init system (systemd / launchd / SCM) and the operator:
//! the init system keeps the launcher alive; the launcher keeps a healthy
//! operator running and owns version swaps + rollback over the on-disk
//! version store. The launcher itself is frozen — it never rewrites its own
//! binary; it is only replaced by a state-preserving reinstall.
//!
//! Layout:
//! - `core/` — the OS-agnostic update state machine, health gate, and the
//!   trait boundary. Must stay platform-blind (see `core`'s module docs);
//!   enforced mechanically by `tests/platform_blind.rs`.
//! - `platform/` — one shim per OS implementing the `core::traits` boundary.
//!
//! Deliberately dependency-light and synchronous: hand-rolled flag parsing,
//! no async runtime — this is the binary that must never break.

mod core;
mod error;
mod platform;

use std::process::ExitCode;

/// Defaults; each is overridable by a flag.
const DEFAULT_DATA_DIR: &str = "/var/lib/alien-operator";
const DEFAULT_PROBATION_SECS: u64 = 90;
const DEFAULT_HEALTH_PORT: u16 = 7799;

const USAGE: &str = "\
alien-launcher — supervises the alien-operator and performs health-gated
binary swaps with last-stable rollback.

USAGE:
    alien-launcher [OPTIONS]

OPTIONS:
    --data-dir <PATH>         Version-store root (default /var/lib/alien-operator)
    --probation-secs <SECS>   Health-gate window after a swap (default 90)
    --health-port <PORT>      Loopback port the operator serves /readyz on (default 7799)
    --console                 Run in the foreground with a Ctrl-C handler instead
                              of under the OS service manager (Windows: bypass the
                              SCM dispatcher; no-op elsewhere). Drives the E2E suite.
    --version                 Print the launcher version and exit
    --help                    Print this help and exit
";

#[derive(Debug, PartialEq)]
struct Args {
    data_dir: std::path::PathBuf,
    probation_secs: u64,
    health_port: u16,
    /// Foreground/console mode (vs OS service manager). Only Windows branches on
    /// it — it selects the SCM-dispatcher bypass; elsewhere it is a no-op.
    console: bool,
}

enum ParseOutcome {
    Run(Args),
    /// --version / --help: print and exit 0.
    PrintAndExit(String),
    Invalid(String),
}

fn parse_args(argv: &[String]) -> ParseOutcome {
    let mut data_dir = std::path::PathBuf::from(DEFAULT_DATA_DIR);
    let mut probation_secs = DEFAULT_PROBATION_SECS;
    let mut health_port = DEFAULT_HEALTH_PORT;
    let mut console = false;

    let mut iter = argv.iter();
    while let Some(flag) = iter.next() {
        let mut value_for = |flag: &str| {
            iter.next()
                .cloned()
                .ok_or_else(|| format!("{flag} requires a value"))
        };
        match flag.as_str() {
            "--version" => {
                return ParseOutcome::PrintAndExit(format!(
                    "alien-launcher {}",
                    env!("CARGO_PKG_VERSION")
                ));
            }
            "--help" => return ParseOutcome::PrintAndExit(USAGE.to_string()),
            "--console" => console = true,
            "--data-dir" => match value_for("--data-dir") {
                Ok(value) => data_dir = std::path::PathBuf::from(value),
                Err(e) => return ParseOutcome::Invalid(e),
            },
            "--probation-secs" => match value_for("--probation-secs")
                .and_then(|v| v.parse::<u64>().map_err(|e| format!("--probation-secs: {e}")))
            {
                Ok(value) => probation_secs = value,
                Err(e) => return ParseOutcome::Invalid(e),
            },
            "--health-port" => match value_for("--health-port")
                .and_then(|v| v.parse::<u16>().map_err(|e| format!("--health-port: {e}")))
            {
                Ok(value) => health_port = value,
                Err(e) => return ParseOutcome::Invalid(e),
            },
            other => return ParseOutcome::Invalid(format!("unknown flag '{other}'")),
        }
    }
    ParseOutcome::Run(Args {
        data_dir,
        probation_secs,
        health_port,
        console,
    })
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        ParseOutcome::Run(args) => args,
        ParseOutcome::PrintAndExit(text) => {
            println!("{text}");
            return ExitCode::SUCCESS;
        }
        ParseOutcome::Invalid(message) => {
            eprintln!("alien-launcher: {message}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    run_supervisor(args)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn run_supervisor(args: Args) -> ExitCode {
    use crate::core::health::UreqProbe;
    use crate::core::state_machine::RunConfig;
    use crate::core::traits::UpdateEnv;

    // Log to stderr; the init system captures it (systemd → journald via the
    // unit's StandardError; launchd → the plist's StandardErrorPath).
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let host = match platform::ActiveHost::new() {
        Ok(host) => host,
        Err(e) => {
            eprintln!("alien-launcher: failed to initialize the service host: {e}");
            return ExitCode::FAILURE;
        }
    };
    let store = match platform::ActiveVersionStore::open(&args.data_dir) {
        Ok(store) => store,
        Err(e) => {
            eprintln!(
                "alien-launcher: failed to open the version store at '{}': {e}",
                args.data_dir.display()
            );
            return ExitCode::FAILURE;
        }
    };
    let mut child = platform::ActiveChildSupervisor::new();
    let probe = UreqProbe::default();

    let config = RunConfig {
        probation_window: std::time::Duration::from_secs(args.probation_secs),
        heartbeat_interval: host.heartbeat_interval(),
        update_env: UpdateEnv {
            health_addr: std::net::SocketAddr::from(([127, 0, 0, 1], args.health_port)),
            launcher_version: env!("CARGO_PKG_VERSION").to_string(),
        },
        ..RunConfig::default()
    };

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        data_dir = %args.data_dir.display(),
        probation_secs = args.probation_secs,
        health_port = args.health_port,
        console = args.console,
        "alien-launcher starting"
    );
    match core::run(&store, &mut child, &probe, &host, &config) {
        Ok(exit) => {
            tracing::info!(?exit, "alien-launcher stopped");
            ExitCode::SUCCESS
        }
        Err(e) => {
            // Fatal (e.g. store corruption the startup classification cannot
            // map): exit nonzero and let the init system respawn us — the
            // fresh classification recovers from whatever is on disk.
            tracing::error!(error = %e, "alien-launcher exiting on a fatal error");
            ExitCode::FAILURE
        }
    }
}

/// Windows: run under the SCM (service_dispatcher → `service_main`), or in the
/// foreground with a Ctrl-C handler when `--console` (what the E2E suite drives).
#[cfg(target_os = "windows")]
fn run_supervisor(args: Args) -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    if args.console {
        match platform::windows::WindowsHost::console() {
            Ok(host) => {
                if run_core_loop(&args, &host) {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
            Err(e) => {
                eprintln!("alien-launcher: failed to install the console Ctrl-C handler: {e}");
                ExitCode::FAILURE
            }
        }
    } else {
        windows_service_entry::run_as_service(args)
    }
}

/// Build the store/child/probe/config and drive `core::run` against a Windows
/// host (console or SCM). Returns whether the loop stopped cleanly — the caller
/// maps that to an `ExitCode` (console) or an SCM stop code (service).
#[cfg(target_os = "windows")]
fn run_core_loop(args: &Args, host: &platform::windows::WindowsHost) -> bool {
    use crate::core::health::UreqProbe;
    use crate::core::state_machine::RunConfig;
    use crate::core::traits::UpdateEnv;

    let store = match platform::ActiveVersionStore::open(&args.data_dir) {
        Ok(store) => store,
        Err(e) => {
            tracing::error!(
                data_dir = %args.data_dir.display(),
                error = %e,
                "failed to open the version store"
            );
            return false;
        }
    };
    let mut child = platform::ActiveChildSupervisor::new();
    let probe = UreqProbe::default();

    let config = RunConfig {
        probation_window: std::time::Duration::from_secs(args.probation_secs),
        heartbeat_interval: host.heartbeat_interval(),
        // The installer writes the operator binary with the platform exe suffix
        // (`alien-operator.exe` on Windows); spawn the matching name.
        operator_binary: format!("alien-operator{}", std::env::consts::EXE_SUFFIX),
        update_env: UpdateEnv {
            health_addr: std::net::SocketAddr::from(([127, 0, 0, 1], args.health_port)),
            launcher_version: env!("CARGO_PKG_VERSION").to_string(),
        },
        ..RunConfig::default()
    };

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        data_dir = %args.data_dir.display(),
        probation_secs = args.probation_secs,
        health_port = args.health_port,
        console = args.console,
        "alien-launcher starting"
    );
    match core::run(&store, &mut child, &probe, host, &config) {
        Ok(exit) => {
            tracing::info!(?exit, "alien-launcher stopped");
            true
        }
        Err(e) => {
            tracing::error!(error = %e, "alien-launcher exiting on a fatal error");
            false
        }
    }
}

/// The SCM entry: `main()` parks the parsed `Args`, hands control to the service
/// dispatcher, and `service_main` (on the service thread) constructs the host,
/// runs the core loop, and reports the final stop status.
#[cfg(target_os = "windows")]
mod windows_service_entry {
    use super::{run_core_loop, Args};
    use crate::platform::windows::WindowsHost;
    use std::ffi::OsString;
    use std::process::ExitCode;
    use std::sync::Mutex;
    use windows_service::{define_windows_service, service_dispatcher};

    // For a SERVICE_WIN32_OWN_PROCESS service the SCM ignores the dispatch-table
    // name; it is only a label for logs.
    const SERVICE_NAME: &str = "alien-launcher";

    // `service_main` is an extern callback that cannot capture, so `main` parks
    // the parsed args here for the service thread to take.
    static SERVICE_ARGS: Mutex<Option<Args>> = Mutex::new(None);

    define_windows_service!(ffi_service_main, service_main);

    pub fn run_as_service(args: Args) -> ExitCode {
        *SERVICE_ARGS.lock().expect("service args lock") = Some(args);
        match service_dispatcher::start(SERVICE_NAME, ffi_service_main) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!(
                    "alien-launcher: could not connect to the service control manager: {e}\n\
                     (run with --console to run in the foreground outside a service)"
                );
                ExitCode::FAILURE
            }
        }
    }

    fn service_main(_scm_args: Vec<OsString>) {
        let args = SERVICE_ARGS
            .lock()
            .expect("service args lock")
            .take()
            .expect("service args parked before dispatch");

        // Registers the SCM control handler and reports StartPending.
        let host = match WindowsHost::service(SERVICE_NAME) {
            Ok(host) => host,
            Err(e) => {
                eprintln!("alien-launcher: failed to register with the SCM: {e}");
                return;
            }
        };
        let clean = run_core_loop(&args, &host);
        // A nonzero stop code trips the SCM recovery config (doc-12 restarts).
        host.report_stopped(if clean { 0 } else { 1 });
    }
}

/// Other platforms have no service shim. Starting the supervisor there is a
/// hard, loud error — never a silent no-op an init system would respawn forever.
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn run_supervisor(_args: Args) -> ExitCode {
    eprintln!(
        "alien-launcher {}: this platform's service shim is not implemented (Linux, macOS, Windows only)",
        env!("CARGO_PKG_VERSION")
    );
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> ParseOutcome {
        parse_args(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn defaults_and_overrides() {
        let ParseOutcome::Run(args) = parse(&[]) else {
            panic!("no flags parses to defaults");
        };
        assert_eq!(args.data_dir, std::path::PathBuf::from(DEFAULT_DATA_DIR));
        assert_eq!(args.probation_secs, DEFAULT_PROBATION_SECS);
        assert_eq!(args.health_port, DEFAULT_HEALTH_PORT);
        assert!(!args.console, "console is off by default");

        let ParseOutcome::Run(args) = parse(&[
            "--data-dir",
            "/tmp/store",
            "--probation-secs",
            "30",
            "--health-port",
            "9000",
            "--console",
        ]) else {
            panic!("full flags parse");
        };
        assert_eq!(args.data_dir, std::path::PathBuf::from("/tmp/store"));
        assert_eq!(args.probation_secs, 30);
        assert_eq!(args.health_port, 9000);
        assert!(args.console, "--console sets console mode");
    }

    #[test]
    fn version_prints_cargo_version() {
        let ParseOutcome::PrintAndExit(text) = parse(&["--version"]) else {
            panic!("--version must print-and-exit");
        };
        assert_eq!(text, format!("alien-launcher {}", env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn invalid_flags_are_loud() {
        assert!(matches!(parse(&["--nope"]), ParseOutcome::Invalid(_)));
        assert!(matches!(parse(&["--health-port"]), ParseOutcome::Invalid(_)));
        assert!(matches!(
            parse(&["--health-port", "not-a-port"]),
            ParseOutcome::Invalid(_)
        ));
        assert!(matches!(
            parse(&["--probation-secs", "-4"]),
            ParseOutcome::Invalid(_)
        ));
    }
}
