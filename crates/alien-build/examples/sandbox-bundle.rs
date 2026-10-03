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

use alien_build::sandbox_bundle::{write_bundle, write_supervised_bundle, AgentSource};

fn main() -> ExitCode {
    let mut arguments: Vec<String> = std::env::args().skip(1).collect();
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

    let result = if supervised {
        match inspect_command(base_image) {
            Ok(command) => {
                write_supervised_bundle(Path::new(destination), base_image, &agent, &command)
            }
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        }
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

fn inspect_command(image: &str) -> Result<alien_core::sandbox_image::SandboxImageCommand, String> {
    let output = std::process::Command::new("docker")
        .args(["image", "inspect", "--format", "{{json .Config}}", image])
        .output()
        .map_err(|error| format!("inspect base image: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "pull the ARM64 base image before packaging: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let config: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("read base image configuration: {error}"))?;
    let mut command = vec![];
    for field in ["Entrypoint", "Cmd"] {
        if let Some(args) = config[field].as_array() {
            for arg in args {
                command.push(
                    arg.as_str()
                        .ok_or_else(|| "image command must contain strings".to_string())?
                        .to_string(),
                );
            }
        }
    }
    let mut env = std::collections::BTreeMap::new();
    if let Some(entries) = config["Env"].as_array() {
        for entry in entries {
            let text = entry
                .as_str()
                .ok_or_else(|| "image environment must contain strings".to_string())?;
            let (key, value) = text
                .split_once('=')
                .ok_or_else(|| "image environment entry must contain '='".to_string())?;
            env.insert(key.to_string(), value.to_string());
        }
    }
    let cwd = config["WorkingDir"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or("/");
    Ok(alien_core::sandbox_image::SandboxImageCommand {
        command,
        env,
        working_directory: cwd.to_string(),
    })
}
