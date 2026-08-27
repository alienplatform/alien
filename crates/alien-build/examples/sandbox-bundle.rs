//! Writes the bundle a Lambda MicroVM image is built from.
//!
//! ```text
//! cargo run -p alien-build --example sandbox-bundle -- <agent-binary> <base-image> <bundle.zip>
//! ```
//!
//! The agent must be an aarch64 Linux binary: MicroVM images accept no other architecture, and
//! an x86 one produces an image that never becomes active.

use std::path::Path;
use std::process::ExitCode;

use alien_build::sandbox_bundle::write_bundle;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [agent, base_image, destination] = arguments.as_slice() else {
        eprintln!("usage: sandbox-bundle <agent-binary> <base-image> <destination.zip>");
        return ExitCode::FAILURE;
    };

    match write_bundle(Path::new(destination), base_image, Path::new(agent)) {
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
