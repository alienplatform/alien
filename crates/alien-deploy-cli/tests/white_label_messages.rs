//! The deploy CLI is shipped under the vendor's name. Sentences it prints (progress, results,
//! errors) must not name the product underneath.
//!
//! The scan reads the non-test source of the CLI and its shared helper crate and looks at every
//! string literal that is prose (it contains a space). Identifiers, paths and environment
//! variable names have no space and are covered by the help-text test in `src/lib.rs` and by
//! the generator tests; this test is about what a person reads.

use std::{fs, path::Path};

/// Prose that may still name the product, each with the reason.
const KNOWN: &[(&str, &str)] = &[
    (
        "Alien Deploy",
        "default display name of the unbranded CLI; a vendor's build replaces it with its own",
    ),
    (
        "alien-deploy sync",
        "follow-up (rename): the sentence hardcodes the unbranded command name; fixing it needs \
         the branded command name passed to the error builders",
    ),
    (
        "alien onboard",
        "follow-up (rename): names the vendor-side command that printed the token",
    ),
    (
        "without alien-deploy builds",
        "follow-up (rename): same hardcoded command name",
    ),
    (
        "alien\\alien-operator.exe",
        "installed file name of the Operator on Windows hosts; existing installs look it up",
    ),
];

fn string_literals(line: &str) -> Vec<String> {
    let mut literals = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut literal = String::new();
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    literal.push(c);
                    if let Some(next) = chars.next() {
                        literal.push(next);
                    }
                }
                '"' => break,
                _ => literal.push(c),
            }
        }
        literals.push(literal);
    }
    literals
}

/// Environment variable names are an existing, scripted contract: they stay.
fn strip_env_var_names(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("ALIEN_") {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let end = tail
            .find(|c: char| !(c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit()))
            .unwrap_or(tail.len());
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).expect("read source directory") {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && !path
                .file_stem()
                .is_some_and(|stem| stem.to_string_lossy().ends_with("_tests"))
        {
            out.push(path);
        }
    }
}

#[test]
fn printed_sentences_never_name_the_product() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    rust_files(&manifest.join("src"), &mut files);
    rust_files(&manifest.join("../alien-cli-common/src"), &mut files);

    let mut offending = Vec::new();
    for file in files {
        let source = fs::read_to_string(&file).expect("read source");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("split always yields one part");
        for (index, line) in production.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            for literal in string_literals(line) {
                if !literal.contains(' ')
                    || !strip_env_var_names(&literal)
                        .to_ascii_lowercase()
                        .contains("alien")
                {
                    continue;
                }
                if KNOWN.iter().any(|(text, _)| literal.contains(text)) {
                    continue;
                }
                offending.push(format!(
                    "{}:{}: {}",
                    file.strip_prefix(manifest).unwrap_or(&file).display(),
                    index + 1,
                    literal
                ));
            }
        }
    }
    assert!(
        offending.is_empty(),
        "the CLI prints sentences that name the product ({}):\n{}",
        offending.len(),
        offending.join("\n")
    );
}
