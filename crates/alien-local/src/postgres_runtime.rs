//! Selects the native server and extension ABI independently of the CLI's linkage.

use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use postgresql_archive::{matcher, Version};
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::OnceCell;

static HOST_RUNTIME: OnceCell<PostgresRuntime> = OnceCell::const_new();
static REGISTER_ARCHIVE_MATCHER: OnceLock<std::result::Result<(), String>> = OnceLock::new();

pub(super) struct PostgresRuntime {
    pub(super) archive_target: &'static str,
    pub(super) extension_os: &'static str,
}

/// Select once per process; server downloads and extension downloads share this choice.
pub(super) async fn native_postgres_runtime() -> Result<&'static PostgresRuntime> {
    let runtime = HOST_RUNTIME.get_or_try_init(detect_runtime).await?;
    REGISTER_ARCHIVE_MATCHER
        .get_or_init(|| {
            matcher::registry::register(
                |url| Ok(url == postgresql_archive::configuration::theseus::URL),
                match_native_archive,
            )
            .map_err(|error| error.to_string())
        })
        .clone()
        .map_err(|reason| {
            AlienError::new(runtime_error(format!(
                "Could not register the native PostgreSQL archive matcher: {reason}"
            )))
        })?;
    Ok(runtime)
}

fn match_native_archive(
    _url: &str,
    name: &str,
    version: &Version,
) -> postgresql_archive::Result<bool> {
    let runtime = HOST_RUNTIME.get().ok_or_else(|| {
        postgresql_archive::Error::UnsupportedMatcher(
            "native PostgreSQL runtime has not been selected".to_string(),
        )
    })?;
    Ok(name == format!("postgresql-{version}-{}.tar.gz", runtime.archive_target))
}

async fn detect_runtime() -> Result<PostgresRuntime> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") | ("linux", "aarch64") => detect_linux_runtime().await,
        ("macos", "aarch64") => Ok(PostgresRuntime {
            archive_target: matcher::target(),
            extension_os: "darwin",
        }),
        ("windows", "x86_64") => Ok(PostgresRuntime {
            archive_target: matcher::target(),
            extension_os: "windows",
        }),
        (os, arch) => Err(AlienError::new(runtime_error(format!(
            "No native PostgreSQL and pgvector runtime is available for {os}-{arch}"
        )))),
    }
}

async fn detect_linux_runtime() -> Result<PostgresRuntime> {
    let (gnu_loader, musl_loader, gnu_target, musl_target) = match std::env::consts::ARCH {
        "x86_64" => (
            "/lib64/ld-linux-x86-64.so.2",
            "/lib/ld-musl-x86_64.so.1",
            "x86_64-unknown-linux-gnu",
            "x86_64-unknown-linux-musl",
        ),
        "aarch64" => (
            "/lib/ld-linux-aarch64.so.1",
            "/lib/ld-musl-aarch64.so.1",
            "aarch64-unknown-linux-gnu",
            "aarch64-unknown-linux-musl",
        ),
        arch => {
            return Err(AlienError::new(runtime_error(format!(
                "No native Linux PostgreSQL runtime is available for {arch}"
            ))));
        }
    };

    // A static MUSL CLI also runs on GNU hosts. Conversely, gcompat's GNU-named
    // interpreter is only a stub, so its pathname alone does not identify glibc.
    if probe_loader(gnu_loader, Loader::Gnu).await? {
        return Ok(PostgresRuntime {
            archive_target: gnu_target,
            extension_os: "linux-gnu",
        });
    }
    if probe_loader(musl_loader, Loader::Musl).await? {
        return Ok(PostgresRuntime {
            archive_target: musl_target,
            extension_os: "linux-musl",
        });
    }
    Err(AlienError::new(runtime_error(format!(
        "Could not identify a native GNU or MUSL runtime using {gnu_loader} and {musl_loader}"
    ))))
}

#[derive(Clone, Copy)]
enum Loader {
    Gnu,
    Musl,
}

async fn probe_loader(path: &str, expected: Loader) -> Result<bool> {
    let mut command = Command::new(path);
    command.arg("--version").kill_on_drop(true);
    let result = tokio::time::timeout(Duration::from_secs(2), command.output())
        .await
        .into_alien_error()
        .context(runtime_error(format!(
            "Runtime loader {path} did not answer its version probe within two seconds"
        )))?;
    let output = match result {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(error).into_alien_error().context(runtime_error(format!(
                "Could not execute runtime loader {path}"
            )));
        }
    };
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(match expected {
        Loader::Gnu => {
            output.status.success() && (text.contains("GLIBC") || text.contains("GNU libc"))
        }
        // The MUSL loader prints its version to stderr and exits nonzero without a program.
        Loader::Musl => text.contains("musl libc"),
    })
}

/// Recover the last server's ABI without executing a potentially incompatible binary.
pub(super) async fn previous_runtime_target(data_dir: &Path) -> Result<String> {
    let unavailable = || {
        runtime_error(format!(
            "Cannot verify the previous PostgreSQL runtime for '{}'. Preserve the data and use its previous runtime to export it before restoring into a new local database",
            data_dir.display()
        ))
    };
    // PostgreSQL writes its executable and arguments here for pg_ctl restart.
    let options = tokio::fs::read_to_string(data_dir.join("postmaster.opts"))
        .await
        .into_alien_error()
        .context(unavailable())?;
    // The executable is unquoted; PostgreSQL quotes each following argument.
    // Split at the first quoted argument so spaces in the executable's path survive.
    let executable = options
        .split_once(" \"")
        .map(|(executable, _)| Path::new(executable))
        .filter(|path| {
            path.is_absolute() && path.file_name().is_some_and(|name| name == "postgres")
        })
        .ok_or_else(|| AlienError::new(unavailable()))?;
    let installation = executable
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| AlienError::new(unavailable()))?;
    // Published server archives include PGXS's build configuration. Reading it
    // also works when the old server's interpreter is absent on this host.
    let makefile = tokio::fs::read_to_string(installation.join("lib/pgxs/src/Makefile.global"))
        .await
        .into_alien_error()
        .context(unavailable())?;
    let variable = |name: &str| {
        makefile
            .lines()
            .filter_map(|line| line.split_once('='))
            .find(|(key, _)| key.trim() == name)
            .map(|(_, value)| value.trim())
    };
    match (variable("host_os"), variable("host_cpu")) {
        (Some(os @ ("linux-gnu" | "linux-musl")), Some(cpu @ ("x86_64" | "aarch64"))) => {
            Ok(format!("{cpu}-unknown-{os}"))
        }
        _ => Err(AlienError::new(unavailable())),
    }
}

fn runtime_error(reason: impl Into<String>) -> ErrorData {
    ErrorData::LocalPostgresRuntimeUnavailable {
        reason: reason.into(),
    }
}
