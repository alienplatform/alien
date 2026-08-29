//! Writes the bundle a Lambda MicroVM image is built from.
//!
//! ```text
//! cargo run -p alien-build --example sandbox-bundle -- --agent-binary <path> <base-image> <bundle.zip>
//! cargo run -p alien-build --example sandbox-bundle -- --agent-image <ref>  <base-image> <bundle.zip>
//! ```
//!
//! `--agent-image` is the shipping path: the bundle carries only a Dockerfile that copies the
//! agent out of the published image. `--agent-binary` embeds a local build for CI/dev instead,
//! and must be aarch64 Linux — MicroVM images accept no other architecture.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use alien_build::sandbox_bundle::{write_bundle, AgentSource};

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let usage = || {
        eprintln!(
            "usage: sandbox-bundle --agent-binary <path>|--agent-image <ref> \
             <base-image> <destination.zip>"
        );
        ExitCode::FAILURE
    };

    let [mode, agent, base_image, destination] = arguments.as_slice() else {
        return usage();
    };
    let agent = match mode.as_str() {
        "--agent-binary" => AgentSource::Binary(PathBuf::from(agent)),
        "--agent-image" => AgentSource::Image(agent.clone()),
        _ => return usage(),
    };

    match write_bundle(Path::new(destination), base_image, &agent) {
        Ok(()) => {
            println!("{destination}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
