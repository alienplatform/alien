//! Builds a hooks crate into the module Alien loads, and its tests into modules the runtime runs
//! as commands.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;

use alien_error::{AlienError, Context, IntoAlienError};

use crate::{ErrorData, Result};

/// The release profile a hooks module ships with: half the size of a default release build, and
/// as fast. A panic aborts the module, after the panic hook `export!` installs has written it.
pub const HOOKS_PROFILE: [(&str, &str); 4] = [
    ("strip", "true"),
    ("lto", "\"fat\""),
    ("codegen-units", "1"),
    ("panic", "\"abort\""),
];

/// Builds `package` from the Cargo workspace at `manifest_path` into a hooks module, with
/// [`HOOKS_PROFILE`], into `target_dir`, and returns the module. `library` is the package's
/// library target's name, which names the file.
///
/// The module's bytes don't depend on where the workspace, the target directory or Cargo's home
/// are: the paths the compiler would embed (in panic locations, for example) are remapped.
///
/// Builds that share a target directory take turns: a build that finds the module fresh can still
/// replace the file another process is about to read.
pub fn build_module(
    manifest_path: &Path,
    package: &str,
    library: &str,
    target_dir: &Path,
) -> Result<Vec<u8>> {
    std::fs::create_dir_all(target_dir)
        .into_alien_error()
        .context(build_failed(
            package,
            format!("{} can't be created", target_dir.display()),
        ))?;
    let lock = File::create(target_dir.join("hooks-module.lock"))
        .into_alien_error()
        .context(build_failed(package, "the build lock can't be created"))?;
    lock.lock()
        .into_alien_error()
        .context(build_failed(package, "the build lock can't be taken"))?;
    let mut command = cargo(&["build", "--release"], manifest_path, target_dir);
    command.args(["--package", package]);
    for (key, value) in HOOKS_PROFILE {
        command
            .arg("--config")
            .arg(format!("profile.release.{key}={value}"));
    }
    let output = command
        .output()
        .into_alien_error()
        .context(build_failed(package, "cargo did not run"))?;
    if !output.status.success() {
        return Err(cargo_failed(package, &output.stderr));
    }
    let module: PathBuf = target_dir
        .join("wasm32-wasip1/release")
        .join(format!("{}.wasm", library.replace('-', "_")));
    std::fs::read(&module)
        .into_alien_error()
        .context(build_failed(
            package,
            format!("{} can't be read", module.display()),
        ))
}

/// Builds the library of `package`, an extension's controller crate, into a controller module: a
/// `cdylib` for wasm32-wasip1 with [`HOOKS_PROFILE`], into `target_dir`, and returns the module.
/// `library` is the library target's name, which names the file.
///
/// Builds that share a target directory take turns, as [`build_module`]'s do.
pub fn build_controller_module(
    manifest_path: &Path,
    package: &str,
    library: &str,
    target_dir: &Path,
) -> Result<Vec<u8>> {
    std::fs::create_dir_all(target_dir)
        .into_alien_error()
        .context(build_failed(
            package,
            format!("{} can't be created", target_dir.display()),
        ))?;
    let lock = File::create(target_dir.join("controller-module.lock"))
        .into_alien_error()
        .context(build_failed(package, "the build lock can't be created"))?;
    lock.lock()
        .into_alien_error()
        .context(build_failed(package, "the build lock can't be taken"))?;
    let mut command = cargo(
        &["rustc", "--release", "--lib", "--crate-type", "cdylib"],
        manifest_path,
        target_dir,
    );
    command.args(["--package", package]);
    for (key, value) in HOOKS_PROFILE {
        command
            .arg("--config")
            .arg(format!("profile.release.{key}={value}"));
    }
    let output = command
        .output()
        .into_alien_error()
        .context(build_failed(package, "cargo did not run"))?;
    if !output.status.success() {
        return Err(cargo_failed(package, &output.stderr));
    }
    let module: PathBuf = target_dir
        .join("wasm32-wasip1/release")
        .join(format!("{}.wasm", library.replace('-', "_")));
    std::fs::read(&module)
        .into_alien_error()
        .context(build_failed(
            package,
            format!("{} can't be read", module.display()),
        ))
}

/// A test binary `cargo test --no-run` built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestBinary {
    /// The name of the target it tests, such as the crate's library or a file in `tests/`
    pub target: String,
    /// The target's kinds, such as `["lib"]` or `["test"]`
    pub kinds: Vec<String>,
    /// The executable, or for wasm32-wasip1 the module
    pub path: PathBuf,
}

/// Builds the tests of `package` for wasm32-wasip1, as `cargo test --no-run` does, and returns
/// the test binaries, each a WASI command module.
pub fn build_test_modules(
    manifest_path: &Path,
    package: &str,
    target_dir: &Path,
) -> Result<Vec<TestBinary>> {
    let output = cargo(&["test", "--no-run"], manifest_path, target_dir)
        .args(["--package", package, "--message-format", "json"])
        .output()
        .into_alien_error()
        .context(build_failed(package, "cargo did not run"))?;
    if !output.status.success() {
        return Err(cargo_failed(package, &output.stderr));
    }
    test_binaries(&output.stdout)
}

/// The test binaries in `cargo --message-format json` output. A test artifact without an
/// executable fails: its tests would not run.
pub fn test_binaries(cargo_json: &[u8]) -> Result<Vec<TestBinary>> {
    cargo_json
        .split(|byte| *byte == b'\n')
        .filter_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
        .filter(|message| {
            message["reason"] == "compiler-artifact" && message["profile"]["test"] == true
        })
        .filter_map(|message| {
            let target = message["target"]["name"].as_str()?.to_string();
            let kinds = message["target"]["kind"]
                .as_array()?
                .iter()
                .filter_map(|kind| kind.as_str().map(str::to_string))
                .collect();
            Some(match message["executable"].as_str() {
                Some(path) => Ok(TestBinary {
                    target,
                    kinds,
                    path: PathBuf::from(path),
                }),
                None => Err(AlienError::new(build_failed(
                    message["package_id"].as_str().unwrap_or_default(),
                    format!("cargo built test target '{target}' without an executable"),
                ))),
            })
        })
        .collect()
}

/// `cargo <subcommand>` for a wasm32-wasip1 build of the workspace at `manifest_path` into
/// `target_dir`.
fn cargo(subcommand: &[&str], manifest_path: &Path, target_dir: &Path) -> Command {
    let workspace = manifest_path.parent().unwrap_or(Path::new("."));
    // `cargo` run in the workspace's directory builds with the toolchain the workspace pins,
    // which may differ from the one running this code and is the one with the wasm32 target.
    let mut command = Command::new("cargo");
    command
        .current_dir(workspace)
        .args(subcommand)
        .args(["--locked", "--target", "wasm32-wasip1"])
        .arg("--manifest-path")
        .arg(manifest_path)
        .arg("--target-dir")
        .arg(target_dir);
    // The calling build's toolchain, flags (a linker, a target) and target directory must not
    // reach this one.
    for variable in [
        "CARGO",
        "RUSTC",
        "RUSTDOC",
        "RUSTUP_TOOLCHAIN",
        "RUSTFLAGS",
        "CARGO_BUILD_TARGET",
        "CARGO_TARGET_DIR",
    ] {
        command.env_remove(variable);
    }
    // These flags replace any the workspace's cargo config sets, so every workspace builds its
    // module the same way. With `--target`, cargo passes them to the module's crates only, not to
    // build scripts or proc macros.
    let mut remaps = vec![
        (workspace.to_path_buf(), "/extension"),
        (target_dir.to_path_buf(), "/target"),
    ];
    if let Some(cargo_home) = cargo_home() {
        remaps.push((cargo_home, "/cargo"));
    }
    let flags: Vec<String> = remaps
        .iter()
        .map(|(from, to)| format!("--remap-path-prefix={}={to}", from.display()))
        .collect();
    command.env("CARGO_ENCODED_RUSTFLAGS", flags.join("\u{1f}"));
    command
}

/// Where cargo keeps the registry and git sources dependencies build from.
fn cargo_home() -> Option<PathBuf> {
    std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")))
}

fn build_failed(package: &str, reason: impl Into<String>) -> ErrorData {
    ErrorData::ModuleBuildFailed {
        package: package.to_string(),
        reason: reason.into(),
    }
}

fn cargo_failed(package: &str, stderr: &[u8]) -> AlienError<ErrorData> {
    AlienError::new(build_failed(
        package,
        format!(
            "cargo could not build it for wasm32-wasip1; `rustup target add wasm32-wasip1` installs the target:\n{}",
            String::from_utf8_lossy(stderr)
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_target_directory_that_can_t_be_created_fails_the_build_of_its_package() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        // A path under a regular file can't be a directory.
        let target_dir = manifest.join("target");
        let error = build_module(&manifest, "hooks", "hooks", &target_dir).unwrap_err();
        assert!(
            matches!(&error.error, Some(ErrorData::ModuleBuildFailed { package, .. }) if package == "hooks"),
            "{error:?}"
        );
    }

    /// A package whose library target is named apart from it builds a file named after the
    /// library, and that is the file read.
    #[test]
    fn a_library_named_apart_from_its_package_is_the_module() {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(workspace.path().join("src")).unwrap();
        std::fs::write(
            workspace.path().join("Cargo.toml"),
            "[package]\nname = \"acme-hooks\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\nname = \"my_hooks_impl\"\ncrate-type = [\"cdylib\"]\n\n[workspace]\n",
        )
        .unwrap();
        // The repository's toolchain, which has the wasm32-wasip1 target.
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rust-toolchain.toml"),
            workspace.path().join("rust-toolchain.toml"),
        )
        .unwrap();
        // The build is `--locked`, as a release's is.
        std::fs::write(
            workspace.path().join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = \"acme-hooks\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(
            workspace.path().join("src/lib.rs"),
            "#[no_mangle]\npub extern \"C\" fn answer() -> i32 { 42 }\n",
        )
        .unwrap();
        let target_dir = workspace.path().join("target");
        let module = build_module(
            &workspace.path().join("Cargo.toml"),
            "acme-hooks",
            "my_hooks_impl",
            &target_dir,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert!(module.starts_with(b"\0asm"));
    }

    #[test]
    fn a_test_artifact_without_an_executable_fails() {
        let output = concat!(
            r#"{"reason":"compiler-artifact","package_id":"path+file:///w/hooks#0.1.0","profile":{"test":true},"target":{"name":"fixtures","kind":["test"]},"executable":null}"#,
            "\n",
            r#"{"reason":"build-finished","success":true}"#,
        );
        let error = test_binaries(output.as_bytes()).unwrap_err();
        assert!(
            matches!(
                &error.error,
                Some(ErrorData::ModuleBuildFailed { package, reason })
                    if package == "path+file:///w/hooks#0.1.0" && reason.contains("fixtures")
            ),
            "{error:?}"
        );
    }

    #[test]
    fn reads_the_test_binaries_cargo_built() {
        let output = concat!(
            r#"{"reason":"compiler-artifact","profile":{"test":false},"target":{"name":"hooks","kind":["cdylib","rlib"]},"executable":null}"#,
            "\n",
            r#"{"reason":"compiler-artifact","profile":{"test":true},"target":{"name":"hooks","kind":["cdylib","rlib"]},"executable":"/t/deps/hooks-1.wasm"}"#,
            "\n",
            r#"{"reason":"compiler-artifact","profile":{"test":true},"target":{"name":"fixtures","kind":["test"]},"executable":"/t/deps/fixtures-2.wasm"}"#,
            "\n",
            r#"{"reason":"build-finished","success":true}"#,
        );
        assert_eq!(
            test_binaries(output.as_bytes()).unwrap(),
            vec![
                TestBinary {
                    target: "hooks".to_string(),
                    kinds: vec!["cdylib".to_string(), "rlib".to_string()],
                    path: PathBuf::from("/t/deps/hooks-1.wasm"),
                },
                TestBinary {
                    target: "fixtures".to_string(),
                    kinds: vec!["test".to_string()],
                    path: PathBuf::from("/t/deps/fixtures-2.wasm"),
                },
            ]
        );
    }
}
