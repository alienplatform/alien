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

#[tokio::main]
async fn main() -> ExitCode {
    let mut arguments: Vec<String> = std::env::args().skip(1).collect();
    let supervised = arguments
        .first()
        .is_some_and(|arg| arg == "--privileged-supervisor");
    if supervised {
        arguments.remove(0);
    }
    let command_uid = if arguments.first().is_some_and(|arg| arg == "--command-uid") {
        arguments.remove(0);
        if arguments.is_empty() {
            eprintln!("--command-uid requires a numeric uid");
            return ExitCode::FAILURE;
        }
        match arguments.remove(0).parse::<u32>() {
            Ok(uid) => Some(uid),
            Err(error) => {
                eprintln!("invalid command uid: {error}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };
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

    let (image, build) = match dockdash::Image::builder()
        .from(base_image)
        .platform("linux", &dockdash::Arch::ARM64)
        .build()
        .await
    {
        Ok(image) => image,
        Err(error) => {
            eprintln!("inspect base image: {error}");
            return ExitCode::FAILURE;
        }
    };
    let identity = match alien_build::sandbox_identity::BaseIdentity::read(&image, base_image).await
    {
        Ok(identity) => identity,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    // Pin the inspected manifest so a tag cannot change users between inspection and AWS build.
    let reference = match base_image.parse::<oci_client::Reference>() {
        Ok(reference) => reference,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let pinned = format!(
        "{}/{}@{}",
        reference.registry(),
        reference.repository(),
        build.resolved_manifest_digest
    );
    let result = if supervised {
        match alien_build::sandbox_identity::image_command(&image) {
            Ok(command) => write_supervised_bundle(
                Path::new(destination),
                &pinned,
                &agent,
                &command,
                &identity,
                command_uid,
            ),
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        }
    } else if command_uid.is_some() {
        eprintln!("--command-uid requires --privileged-supervisor");
        return ExitCode::FAILURE;
    } else {
        write_bundle(Path::new(destination), &pinned, &agent, &identity)
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
