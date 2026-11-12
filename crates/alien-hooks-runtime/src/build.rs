//! Builds a hooks crate into the module Alien loads, and its tests into modules the runtime runs
//! as commands.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The release profile a hooks module ships with: half the size of a default release build, and
/// as fast. A panic aborts the module, after the panic hook `export!` installs has written it.
pub const HOOKS_PROFILE: [(&str, &str); 4] = [
    ("strip", "true"),
    ("lto", "\"fat\""),
    ("codegen-units", "1"),
    ("panic", "\"abort\""),
];

/// Builds `package` from the Cargo workspace at `manifest_path` into a hooks module, with
/// [`HOOKS_PROFILE`], into `target_dir`, and returns the module.
///
/// The module's bytes don't depend on where the workspace, the target directory or Cargo's home
/// are: the paths the compiler would embed (in panic locations, for example) are remapped.
///
/// Builds that share a target directory take turns: a build that finds the module fresh can still
/// replace the file another process is about to read.
pub fn build_module(
    manifest_path: &Path,
    package: &str,
    target_dir: &Path,
) -> Result<Vec<u8>, String> {
    std::fs::create_dir_all(target_dir)
        .map_err(|error| format!("{} can't be created: {error}", target_dir.display()))?;
    let lock = File::create(target_dir.join("hooks-module.lock"))
        .map_err(|error| format!("the build lock can't be created: {error}"))?;
    lock.lock()
        .map_err(|error| format!("the build lock can't be taken: {error}"))?;
    let mut command = cargo(&["build", "--release"], manifest_path, target_dir);
    command.args(["--package", package]);
    for (key, value) in HOOKS_PROFILE {
        command
            .arg("--config")
            .arg(format!("profile.release.{key}={value}"));
    }
    let output = command
        .output()
        .map_err(|error| format!("cargo did not run: {error}"))?;
    if !output.status.success() {
        return Err(failed(package, &output.stderr));
    }
    let module: PathBuf = target_dir
        .join("wasm32-wasip1/release")
        .join(format!("{}.wasm", package.replace('-', "_")));
    std::fs::read(&module).map_err(|error| format!("{} can't be read: {error}", module.display()))
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
) -> Result<Vec<TestBinary>, String> {
    let output = cargo(&["test", "--no-run"], manifest_path, target_dir)
        .args(["--package", package, "--message-format", "json"])
        .output()
        .map_err(|error| format!("cargo did not run: {error}"))?;
    if !output.status.success() {
        return Err(failed(package, &output.stderr));
    }
    Ok(test_binaries(&output.stdout))
}

/// The test binaries in `cargo --message-format json` output.
pub fn test_binaries(cargo_json: &[u8]) -> Vec<TestBinary> {
    cargo_json
        .split(|byte| *byte == b'\n')
        .filter_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
        .filter(|message| {
            message["reason"] == "compiler-artifact" && message["profile"]["test"] == true
        })
        .filter_map(|message| {
            Some(TestBinary {
                target: message["target"]["name"].as_str()?.to_string(),
                kinds: message["target"]["kind"]
                    .as_array()?
                    .iter()
                    .filter_map(|kind| kind.as_str().map(str::to_string))
                    .collect(),
                path: PathBuf::from(message["executable"].as_str()?),
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

fn failed(package: &str, stderr: &[u8]) -> String {
    format!(
        "building {package} for wasm32-wasip1 failed; `rustup target add wasm32-wasip1` installs the target:\n{}",
        String::from_utf8_lossy(stderr)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
            test_binaries(output.as_bytes()),
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
