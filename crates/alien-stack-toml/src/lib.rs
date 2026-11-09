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

mod canonical;
mod env_file;
mod error;
mod node;
mod resolve;
mod schema;
mod source;
mod sugar;
mod suggest;
mod units;

use std::path::Path;

use alien_core::Stack;
use alien_error::{Context, IntoAlienError};
use toml::de::DeTable;

pub use error::{ErrorData, Result};
pub use resolve::DEFAULT_CLUSTER_ID;
use source::SourceFile;

/// What the tests that generate the editor schema (`schema/alien.toml.json`) read from the
/// resolver, so the schema cannot drift from the keys the resolver accepts.
#[doc(hidden)]
pub mod authoring {
    pub use crate::canonical::{allowed_keys, author_key, split_unit, Vocabulary};
    pub use crate::sugar::vocabulary;
    pub use crate::units::UnitSuffix;
}

/// The JSON schema editors check an `alien.toml` against, which the file's
/// `#:schema` line points at.
pub const EDITOR_SCHEMA: &str = include_str!("../schema/alien.toml.json");

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
    let document = DeTable::parse(text)
        .map_err(|error| file.error(error.span().unwrap_or(0..0), error.message().trim_end()))?;
    resolve::resolve(node::from_document(document), &file, base_dir)
}

/// The resolved stack of the `alien.toml` file at `path`, as the JSON the deployment
/// engine reads. It is every default and expansion made visible.
pub fn expand(path: &Path) -> Result<serde_json::Value> {
    let stack = load(path)?;
    Ok(serde_json::to_value(stack).expect("a stack serializes"))
}
