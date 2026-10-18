//! Writes the bundle a Lambda MicroVM image is built from.
//!
//! ```text
//! cargo run -p alien-build --example sandbox-bundle -- --agent-binary <path> <base-image> <bundle.zip>
//! cargo run -p alien-build --example sandbox-bundle -- --agent-image <ref>  <base-image> <bundle.zip>
//! cargo run -p alien-build --example sandbox-bundle -- --sandbox <sandbox.json> --agent-binary <path> <base-image> <bundle.zip>
//! ```
//!
//! `--agent-image` is the shipping path: the bundle carries only a Dockerfile that copies the
//! agent out of the published image. `--agent-binary` embeds a local build for CI/dev instead,
//! and must be aarch64 Linux — MicroVM images accept no other architecture.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use alien_build::sandbox_bundle::{write_bundle, write_declared_bundle, AgentSource};

fn main() -> ExitCode {
    let mut arguments: Vec<String> = std::env::args().skip(1).collect();
    let declaration = if arguments.first().is_some_and(|arg| arg == "--sandbox") {
        if arguments.len() < 2 {
            eprintln!("--sandbox requires a sandbox JSON file");
            return ExitCode::FAILURE;
        }
        let path = arguments.remove(1);
        arguments.remove(0);
        match std::fs::read(path)
            .map_err(|error| error.to_string())
            .and_then(|bytes| {
                serde_json::from_slice::<alien_core::Sandbox>(&bytes)
                    .map_err(|error| error.to_string())
            }) {
            Ok(sandbox) => Some(sandbox),
            Err(error) => {
                eprintln!("read sandbox declaration: {error}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };
    let supervised = arguments
        .first()
        .is_some_and(|arg| arg == "--privileged-supervisor");
    if supervised {
        arguments.remove(0);
    }
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

    let egress = declaration
        .map(|sandbox| sandbox.egress)
        .or_else(|| supervised.then_some(alien_core::SandboxEgress::Internet));
    let result = if let Some(egress) = egress {
        let config = if matches!(egress, alien_core::SandboxEgress::Internet) {
            match inspect_config(base_image) {
                Ok(config) => Some(config),
                Err(error) => {
                    eprintln!("{error}");
                    return ExitCode::FAILURE;
                }
            }
        } else {
            None
        };
        write_declared_bundle(
            Path::new(destination),
            base_image,
            &agent,
            &egress,
            config.as_ref(),
        )
    } else {
        write_bundle(Path::new(destination), base_image, &agent)
    };
    match result {
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

fn inspect_config(image: &str) -> Result<oci_client::config::Config, String> {
    let output = std::process::Command::new("docker")
        .args(["image", "inspect", "--format", "{{json .Config}}", image])
        .output()
        .map_err(|error| format!("inspect base image: {error}"))?;
    if !output.status.success() {
        return Err(format!("pull the base image before packaging: {image}"));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("read base image configuration: {error}"))
}
