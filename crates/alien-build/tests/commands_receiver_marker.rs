//! `build_stack` records which built Containers and Daemons have a command
//! receiver that reads the capability file. Real Rust builds of small apps.

use alien_build::build_stack;
use alien_build::settings::{BuildSettings, PlatformBuildSettings};
use alien_core::permissions::PermissionProfile;
use alien_core::{
    Container, ContainerCode, Daemon, DaemonCode, ResourceLifecycle, Stack, ToolchainConfig,
};
use std::path::Path;
use tempfile::tempdir;

fn write_receiver_crate(dir: &Path, marked: bool) {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let metadata = if marked {
        "\n[package.metadata.alien]\ncommandsCapabilityFile = true\n"
    } else {
        ""
    };
    std::fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"alien-commands\"\nversion = \"1.0.0\"\nedition = \"2021\"\n{metadata}"
        ),
    )
    .unwrap();
    std::fs::write(dir.join("src/lib.rs"), "").unwrap();
}

/// An app crate named `name`; `receiver` is the path of the receiver crate it links.
fn write_app(dir: &Path, name: &str, receiver: Option<&Path>) {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let dependency = receiver
        .map(|path| {
            format!(
                "\n[dependencies]\nalien-commands = {{ path = {:?} }}\n",
                path.display().to_string()
            )
        })
        .unwrap_or_default();
    std::fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n{dependency}"
        ),
    )
    .unwrap();
    std::fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();
}

fn container(id: &str, src: &Path, commands: bool, hand_written_marker: bool) -> Container {
    let mut container = Container::new(id.to_string())
        .code(ContainerCode::Source {
            src: src.display().to_string(),
            toolchain: ToolchainConfig::Rust {
                binary_name: id.to_string(),
            },
        })
        .cpu(alien_core::ResourceSpec {
            min: "0.5".to_string(),
            desired: "1".to_string(),
        })
        .memory(alien_core::ResourceSpec {
            min: "512Mi".to_string(),
            desired: "1Gi".to_string(),
        })
        .commands_enabled(commands)
        .permissions("execution".to_string())
        .build();
    container.commands_capability_file = hand_written_marker;
    container
}

fn daemon(id: &str, src: &Path) -> Daemon {
    Daemon::new(id.to_string())
        .code(DaemonCode::Source {
            src: src.display().to_string(),
            toolchain: ToolchainConfig::Rust {
                binary_name: id.to_string(),
            },
        })
        .commands_enabled(true)
        .permissions("execution".to_string())
        .build()
}

#[tokio::test]
async fn build_marks_only_receivers_that_read_the_capability_file() {
    std::env::set_var("ALIEN_SKIP_DEPENDENCY_INSTALL", "1");
    let root = tempdir().unwrap();
    let marked_receiver = root.path().join("marked-receiver");
    let older_receiver = root.path().join("older-receiver");
    write_receiver_crate(&marked_receiver, true);
    write_receiver_crate(&older_receiver, false);

    write_app(
        &root.path().join("current"),
        "current",
        Some(&marked_receiver),
    );
    write_app(&root.path().join("older"), "older", Some(&older_receiver));
    write_app(&root.path().join("plain"), "plain", None);
    write_app(&root.path().join("agent"), "agent", Some(&marked_receiver));
    write_app(
        &root.path().join("old-agent"),
        "old-agent",
        Some(&older_receiver),
    );

    let stack = Stack::new("marker-stack".to_string())
        .permission("execution", PermissionProfile::new())
        .add(
            container("current", &root.path().join("current"), true, false),
            ResourceLifecycle::Live,
        )
        .add(
            container("older", &root.path().join("older"), true, false),
            ResourceLifecycle::Live,
        )
        // The stack definition cannot claim the marker: only the build sets it.
        .add(
            container("plain", &root.path().join("plain"), true, true),
            ResourceLifecycle::Live,
        )
        .add(
            daemon("agent", &root.path().join("agent")),
            ResourceLifecycle::Live,
        )
        .add(
            daemon("old-agent", &root.path().join("old-agent")),
            ResourceLifecycle::Live,
        )
        .build();

    let output = tempdir().unwrap();
    let settings = BuildSettings {
        output_directory: output.path().display().to_string(),
        platform: PlatformBuildSettings::Test {},
        targets: None,
        cache_url: None,
        override_base_image: None,
        debug_mode: false,
        rebuild: false,
        pull_base_images: false,
    };
    let built = build_stack(stack, &settings)
        .await
        .expect("build should succeed");

    let marked = |id: &str| built.command_receiver_reads_capability_file(id);
    assert!(marked("current"), "linked the marked receiver");
    assert!(marked("agent"), "daemons are marked the same way");
    assert!(!marked("older"), "an older alien-commands has no marker");
    assert!(
        !marked("plain"),
        "no receiver, and a hand-written claim is dropped"
    );
    assert!(!marked("old-agent"));
    let mut unmarked = built.command_receivers_without_capability_file();
    unmarked.sort();
    assert_eq!(unmarked, ["old-agent", "older", "plain"]);

    // The marker is part of the release the manager and operator read.
    let json = serde_json::to_value(&built).unwrap();
    let text = json.to_string();
    assert_eq!(text.matches("\"commandsCapabilityFile\":true").count(), 2);
}
