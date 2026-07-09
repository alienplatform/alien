//! Mechanical guard: the launcher's `src/core/` must stay platform-blind.
//!
//! The core is written once and must compile and behave identically for
//! Linux, macOS, and Windows; all platform behavior belongs behind the
//! `core::traits` boundary in `src/platform/`. This test scans every source
//! file under `src/core/` for tokens that would leak a platform into the
//! core. It lives OUTSIDE `src/core/` on purpose — its own forbidden-token
//! list would otherwise trip the scan.
//!
//! Comments are stripped before matching so module docs may *describe* the
//! rule (and name the forbidden crates) without violating it.

use std::path::{Path, PathBuf};

/// Tokens that must never appear in non-comment core source. Kept as
/// substrings so `use nix::...`, `nix::sys::...`, and `extern crate nix`
/// are all caught.
const FORBIDDEN: &[&str] = &[
    "std::os::unix",
    "std::os::windows",
    "nix::",
    "rustix::",
    "libc::",
    "windows_sys",
    "windows_service",
    "command_group",
    "sd_notify",
    "junction::",
    "cfg(unix)",
    "cfg(windows)",
    "cfg(target_os",
];

/// Strip `//`-style comments (line, doc, inner-doc). Good enough for this
/// codebase: the core contains no block comments or string literals holding
/// `//` (and a false *positive* here would only make the guard stricter).
fn strip_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| match line.find("//") {
            Some(idx) => &line[..idx],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", dir.display()))
    {
        let path = entry.expect("dir entry should be readable").path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn core_is_platform_blind() {
    let core_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/core");
    let mut files = Vec::new();
    collect_rs_files(&core_dir, &mut files);
    assert!(
        !files.is_empty(),
        "no source files found under {} — the scan target moved?",
        core_dir.display()
    );

    let mut violations = Vec::new();
    for file in &files {
        let source = std::fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", file.display()));
        let code = strip_comments(&source);
        for token in FORBIDDEN {
            if code.contains(token) {
                violations.push(format!("{}: contains `{token}`", file.display()));
            }
        }

        // The state machine must drive the VersionStore trait exclusively —
        // no direct filesystem access in its production code. Its #[cfg(test)]
        // module legitimately inspects disk state, so the scan covers only the
        // code BEFORE the first `#[cfg(test)]` — which relies on the Rust
        // convention that the tests module ends the file. Don't add production
        // code below the tests module; this guard would not see it.
        if file.file_name().is_some_and(|name| name == "state_machine.rs") {
            let production = code.split("#[cfg(test)]").next().unwrap_or(&code);
            if production.contains("std::fs") {
                violations.push(format!(
                    "{}: production code calls std::fs — go through VersionStore",
                    file.display()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "src/core/ must stay platform-blind — move platform code behind the \
         core::traits boundary into src/platform/.\nViolations:\n{}",
        violations.join("\n")
    );
}
