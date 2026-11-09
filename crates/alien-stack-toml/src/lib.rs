//! `alien.toml`: an Alien stack written as a declarative TOML file.
//!
//! It is another way to write the stack an `alien.ts` file describes, with the same
//! resources and names, for projects that do not otherwise use TypeScript. The CLI
//! parses it directly, so loading it runs no code and needs no JavaScript runtime.
//!
//! ```toml
//! #:schema https://alien.dev/schemas/alien.toml.json
//! name = "acme-agent"
//! platforms = ["machines"]
//!
//! [daemon.agent]
//! build = { toolchain = "rust", binary-name = "acme-agent" }
//! health-check = { port = 8080, path = "/health" }
//! ```
//!
//! Errors point at the line and column of the offending value. Unknown keys are errors.

mod document;
mod env_file;
mod error;
mod resolve;
mod source;
mod suggest;

use std::path::Path;

use alien_core::Stack;
use alien_error::{Context, IntoAlienError};

pub use document::{Document, DAEMON_KEYS};
pub use error::{ErrorData, Result};
pub use resolve::DEFAULT_CLUSTER_ID;
use source::SourceFile;

/// Loads the stack in the `alien.toml` file at `path`.
pub fn load(path: &Path) -> Result<Stack> {
    let text =
        std::fs::read_to_string(path)
            .into_alien_error()
            .context(ErrorData::FileUnreadable {
                file: path.display().to_string(),
            })?;
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    parse(&text, &path.display().to_string(), base_dir)
}

/// Resolves `alien.toml` source text into a stack.
///
/// `file_name` is shown in errors. `base_dir` is the directory relative paths in the file
/// (such as `env-file`) are resolved against.
pub fn parse(text: &str, file_name: &str, base_dir: &Path) -> Result<Stack> {
    let file = SourceFile {
        name: file_name.to_string(),
        text,
    };
    let document: Document = toml::from_str(text)
        .map_err(|error| file.error(error.span().unwrap_or(0..0), toml_message(&error)))?;
    resolve::resolve(document, &file, base_dir)
}

/// The JSON schema of `alien.toml`, for editor completion (Taplo, Even Better TOML).
///
/// Object keys are sorted, so the schema is the same whether or not serde_json's
/// `preserve_order` feature is enabled in the build.
pub fn json_schema() -> serde_json::Value {
    let mut schema = schemars::schema_for!(Document);
    schema.schema.metadata().id = Some("https://alien.dev/schemas/alien.toml.json".to_string());
    sort_keys(serde_json::to_value(schema).expect("a JSON schema serializes"))
}

fn sort_keys(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(object) => {
            let mut entries: Vec<_> = object.into_iter().collect();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            serde_json::Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, sort_keys(value)))
                    .collect(),
            )
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(sort_keys).collect())
        }
        other => other,
    }
}

/// The parser's message without its own location prefix, with a "did you mean" hint after
/// an unknown key or value that is close to a valid one.
fn toml_message(error: &toml::de::Error) -> String {
    let message = error.message().trim_end().to_string();
    let Some((name_end, unknown, expected)) = unknown_and_expected(&message) else {
        return message;
    };
    let hint = suggest::hint(unknown, expected);
    format!("{}{hint}{}", &message[..name_end], &message[name_end..])
}

/// For serde's "unknown field `x`, expected one of `a`, `b`": where the rejected name ends,
/// the name, and the valid ones.
fn unknown_and_expected(message: &str) -> Option<(usize, &str, Vec<&str>)> {
    let prefix = ["unknown field `", "unknown variant `"]
        .into_iter()
        .find(|prefix| message.starts_with(prefix))?;
    let rest = &message[prefix.len()..];
    let (unknown, expected) = rest.split_once('`')?;
    let name_end = prefix.len() + unknown.len() + 1;
    let expected = expected.split('`').skip(1).step_by(2).collect();
    Some((name_end, unknown, expected))
}
