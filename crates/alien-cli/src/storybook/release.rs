//! Release command simulation functions for the storybook

use super::{create_build_error, create_config_error};
use alien_cli::tui::{ReleaseResult, ReleaseUiComponent, ReleaseUiEvent, ReleaseUiProps};
use alien_core::{AlienEvent, EventChange, EventState, PushProgress};
use clap::Subcommand;
use std::{thread, time::Duration};

#[derive(Subcommand)]
pub enum ReleaseDemo {
    /// Loading configuration
    #[command(name = "1")]
    Config,
    /// Configuration error
    #[command(name = "1a")]
    ConfigError,
    /// Pushing images
    #[command(name = "2")]
    Push,
    /// Push failed
    #[command(name = "2a")]
    PushFailed,
    /// Single function push
    #[command(name = "2b")]
    SingleFunction,
    /// Many functions push
    #[command(name = "2c")]
    ManyFunctions,
    /// Creating release
    #[command(name = "3")]
    CreateRelease,
    /// Release success
    #[command(name = "4")]
    Success,
}

impl ReleaseDemo {
    pub fn run(self) -> color_eyre::Result<()> {
        let (platforms, project_name, simulate_fn): (
            Vec<String>,
            &str,
            fn(std::sync::mpsc::Sender<ReleaseUiEvent>),
        ) = match self {
            Self::Config => (vec!["aws".to_string()], "my-project", simulate_release_1),
            Self::ConfigError => (vec!["aws".to_string()], "my-project", simulate_release_1a),
            Self::Push => (vec!["aws".to_string()], "my-project", simulate_release_2),
            Self::PushFailed => (vec!["aws".to_string()], "my-project", simulate_release_2a),
            Self::SingleFunction => (vec!["aws".to_string()], "simple-app", simulate_release_2b),
            Self::ManyFunctions => (
                vec!["aws".to_string(), "gcp".to_string()],
                "large-app",
                simulate_release_2c,
            ),
            Self::CreateRelease => (vec!["aws".to_string()], "my-project", simulate_release_3),
            Self::Success => (vec!["aws".to_string()], "my-project", simulate_release_4),
        };

        run_release_demo_impl(platforms, project_name, simulate_fn)
    }
}

/// Run a release demo with the given platforms and project name
fn run_release_demo_impl(
    platforms: Vec<String>,
    project_name: &str,
    simulate_fn: fn(std::sync::mpsc::Sender<ReleaseUiEvent>),
) -> color_eyre::Result<()> {
    // Create the ReleaseUiComponent with props
    let props = ReleaseUiProps {
        platforms,
        project_name: project_name.to_string(),
        on_result: None,
        on_cancel: None,
    };

    let mut ui_component = ReleaseUiComponent::new(props);

    // Start the component and get the event sender
    let ui_event_tx = ui_component
        .start()
        .map_err(|e| color_eyre::eyre::eyre!("Failed to start UI component: {}", e.message))?;

    // Simulate the demo by sending events in a separate thread
    let demo_tx = ui_event_tx.clone();
    thread::spawn(move || {
        simulate_fn(demo_tx);
    });

    // Run the UI component event loop (this will handle the demo automatically)
    ui_component
        .run_event_loop()
        .map_err(|e| color_eyre::eyre::eyre!("UI component error: {}", e.message))
}

/// Simulate release-1: Loading configuration
pub fn simulate_release_1(tx: std::sync::mpsc::Sender<ReleaseUiEvent>) {
    thread::sleep(Duration::from_millis(100));

    // Send LoadingConfiguration started event
    let event_change = EventChange::Created {
        id: "config-1".to_string(),
        parent_id: None,
        event: AlienEvent::LoadingConfiguration,
        state: EventState::Started,
        created_at: chrono::Utc::now(),
    };
    let _ = tx.send(ReleaseUiEvent::AlienEventChange(event_change));

    // Keep running for demo duration
    thread::sleep(Duration::from_millis(2000));
}

/// Simulate release-1a: Configuration loading failed
pub fn simulate_release_1a(tx: std::sync::mpsc::Sender<ReleaseUiEvent>) {
    thread::sleep(Duration::from_millis(100));

    // Send LoadingConfiguration started event
    let event_change = EventChange::Created {
        id: "config-1".to_string(),
        parent_id: None,
        event: AlienEvent::LoadingConfiguration,
        state: EventState::Started,
        created_at: chrono::Utc::now(),
    };
    let _ = tx.send(ReleaseUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(300));

    // Send configuration failed event
    let event_change = EventChange::StateChanged {
        id: "config-1".to_string(),
        new_state: EventState::Failed { error: None },
        updated_at: chrono::Utc::now(),
    };
    let _ = tx.send(ReleaseUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(100));

    // Send release failed result
    let config_error = create_config_error();
    let _ = tx.send(ReleaseUiEvent::ReleaseFinished(Err(config_error)));

    thread::sleep(Duration::from_millis(2000));
}

/// Simulate release-2: Pushing images (4 functions)
pub fn simulate_release_2(tx: std::sync::mpsc::Sender<ReleaseUiEvent>) {
    thread::sleep(Duration::from_millis(100));

    // Configuration success
    send_config_success(&tx);

    thread::sleep(Duration::from_millis(200));

    // Start pushing stack
    let event_change = EventChange::Created {
        id: "push-stack-1".to_string(),
        parent_id: None,
        created_at: chrono::Utc::now(),
        event: AlienEvent::PushingStack {
            stack: "my-project".to_string(),
            platform: "aws".to_string(),
        },
        state: EventState::Started,
    };
    let _ = tx.send(ReleaseUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(100));

    // Push 4 functions
    let functions = [
        "api-handler",
        "background-worker",
        "data-processor",
        "notification-service",
    ];

    for (i, function_name) in functions.iter().enumerate() {
        let event_change = EventChange::Created {
            id: format!("function-{}", i),
            parent_id: Some("push-stack-1".to_string()),
            created_at: chrono::Utc::now(),
            event: AlienEvent::PushingResource {
                resource_name: function_name.to_string(),
                resource_type: "function".to_string(),
            },
            state: EventState::Started,
        };
        let _ = tx.send(ReleaseUiEvent::AlienEventChange(event_change));

        thread::sleep(Duration::from_millis(50));

        // Show pushing progress for variety
        match i {
            0 => send_function_pushing(&tx, &format!("function-{}", i), function_name, 19, 20),
            1 => send_function_pushing(&tx, &format!("function-{}", i), function_name, 5, 15),
            2 => send_function_pushing(&tx, &format!("function-{}", i), function_name, 10, 25),
            _ => send_function_pushing(&tx, &format!("function-{}", i), function_name, 2, 30),
        }
    }

    thread::sleep(Duration::from_millis(2000));
}

/// Simulate release-2a: Push failed
pub fn simulate_release_2a(tx: std::sync::mpsc::Sender<ReleaseUiEvent>) {
    thread::sleep(Duration::from_millis(100));

    // Configuration success
    send_config_success(&tx);

    thread::sleep(Duration::from_millis(200));

    // Start pushing
    let event_change = EventChange::Created {
        id: "push-stack-1".to_string(),
        parent_id: None,
        created_at: chrono::Utc::now(),
        event: AlienEvent::PushingStack {
            stack: "my-project".to_string(),
            platform: "aws".to_string(),
        },
        state: EventState::Started,
    };
    let _ = tx.send(ReleaseUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(100));

    // Start pushing single function
    let event_change = EventChange::Created {
        id: "function-1".to_string(),
        parent_id: Some("push-stack-1".to_string()),
        created_at: chrono::Utc::now(),
        event: AlienEvent::PushingResource {
            resource_name: "data-analyzer".to_string(),
            resource_type: "function".to_string(),
        },
        state: EventState::Started,
    };
    let _ = tx.send(ReleaseUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(200));

    // Show some progress
    send_function_pushing(&tx, "function-1", "data-analyzer", 5, 20);

    thread::sleep(Duration::from_millis(300));

    // Function push fails
    let event_change = EventChange::StateChanged {
        id: "function-1".to_string(),
        new_state: EventState::Failed { error: None },
        updated_at: chrono::Utc::now(),
    };
    let _ = tx.send(ReleaseUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(100));

    // Send release error result
    let push_error = create_build_error();
    let _ = tx.send(ReleaseUiEvent::ReleaseFinished(Err(push_error)));

    thread::sleep(Duration::from_millis(2000));
}

/// Simulate release-2b: Single function push
pub fn simulate_release_2b(tx: std::sync::mpsc::Sender<ReleaseUiEvent>) {
    thread::sleep(Duration::from_millis(100));

    // Configuration success
    send_config_success(&tx);

    thread::sleep(Duration::from_millis(200));

    // Start pushing
    let event_change = EventChange::Created {
        id: "push-stack-1".to_string(),
        parent_id: None,
        created_at: chrono::Utc::now(),
        event: AlienEvent::PushingStack {
            stack: "simple-app".to_string(),
            platform: "aws".to_string(),
        },
        state: EventState::Started,
    };
    let _ = tx.send(ReleaseUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(100));

    // Push single function
    let event_change = EventChange::Created {
        id: "function-1".to_string(),
        parent_id: Some("push-stack-1".to_string()),
        created_at: chrono::Utc::now(),
        event: AlienEvent::PushingResource {
            resource_name: "api-handler".to_string(),
            resource_type: "function".to_string(),
        },
        state: EventState::Started,
    };
    let _ = tx.send(ReleaseUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(100));

    // Show pushing progress
    send_function_pushing(&tx, "function-1", "api-handler", 10, 25);

    thread::sleep(Duration::from_millis(2000));
}

/// Simulate release-2c: Many functions push
pub fn simulate_release_2c(tx: std::sync::mpsc::Sender<ReleaseUiEvent>) {
    thread::sleep(Duration::from_millis(100));

    // Configuration success
    send_config_success(&tx);

    thread::sleep(Duration::from_millis(200));

    // Start pushing
    let event_change = EventChange::Created {
        id: "push-stack-1".to_string(),
        parent_id: None,
        created_at: chrono::Utc::now(),
        event: AlienEvent::PushingStack {
            stack: "large-app".to_string(),
            platform: "aws".to_string(),
        },
        state: EventState::Started,
    };
    let _ = tx.send(ReleaseUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(100));

    // Push 10 functions
    let functions = [
        "api-gateway",
        "user-auth",
        "user-profile",
        "order-service",
        "payment-processor",
        "inventory-manager",
        "notification-service",
        "email-service",
        "analytics-collector",
        "report-generator",
    ];

    for (i, function_name) in functions.iter().enumerate() {
        let event_change = EventChange::Created {
            id: format!("function-{}", i),
            parent_id: Some("push-stack-1".to_string()),
            created_at: chrono::Utc::now(),
            event: AlienEvent::PushingResource {
                resource_name: function_name.to_string(),
                resource_type: "function".to_string(),
            },
            state: EventState::Started,
        };
        let _ = tx.send(ReleaseUiEvent::AlienEventChange(event_change));

        thread::sleep(Duration::from_millis(30));

        // Show different push states
        match i {
            0..=2 => {
                // First 3: completed
                send_function_pushing(&tx, &format!("function-{}", i), function_name, 20, 20);
                thread::sleep(Duration::from_millis(50));
                let event_change = EventChange::StateChanged {
                    id: format!("function-{}", i),
                    new_state: EventState::Success,
                    updated_at: chrono::Utc::now(),
                };
                let _ = tx.send(ReleaseUiEvent::AlienEventChange(event_change));
            }
            3..=6 => {
                // Next 4: pushing at various stages
                send_function_pushing(
                    &tx,
                    &format!("function-{}", i),
                    function_name,
                    (i * 3) as u64,
                    25,
                );
            }
            _ => {
                // Last 3: queued
            }
        }
    }

    thread::sleep(Duration::from_millis(2000));
}

/// Simulate release-3: Creating release on platform
pub fn simulate_release_3(tx: std::sync::mpsc::Sender<ReleaseUiEvent>) {
    thread::sleep(Duration::from_millis(100));

    // Configuration success
    send_config_success(&tx);

    thread::sleep(Duration::from_millis(200));

    // Push and complete functions quickly
    let event_change = EventChange::Created {
        id: "push-stack-1".to_string(),
        parent_id: None,
        created_at: chrono::Utc::now(),
        event: AlienEvent::PushingStack {
            stack: "my-project".to_string(),
            platform: "aws".to_string(),
        },
        state: EventState::Success,
    };
    let _ = tx.send(ReleaseUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(200));

    // Now creating release on platform
    let event_change = EventChange::Created {
        id: "create-release-1".to_string(),
        parent_id: None,
        created_at: chrono::Utc::now(),
        event: AlienEvent::CreatingRelease {
            project: "my-project".to_string(),
        },
        state: EventState::Started,
    };
    let _ = tx.send(ReleaseUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(2000));
}

/// Simulate release-4: Release success
pub fn simulate_release_4(tx: std::sync::mpsc::Sender<ReleaseUiEvent>) {
    thread::sleep(Duration::from_millis(100));

    // Configuration success
    send_config_success(&tx);

    thread::sleep(Duration::from_millis(200));

    // Push functions
    let event_change = EventChange::Created {
        id: "push-stack-1".to_string(),
        parent_id: None,
        created_at: chrono::Utc::now(),
        event: AlienEvent::PushingStack {
            stack: "my-project".to_string(),
            platform: "aws".to_string(),
        },
        state: EventState::Started,
    };
    let _ = tx.send(ReleaseUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(100));

    let functions = ["api-handler", "background-worker", "data-processor"];
    for (i, function_name) in functions.iter().enumerate() {
        let event_change = EventChange::Created {
            id: format!("function-{}", i),
            parent_id: Some("push-stack-1".to_string()),
            created_at: chrono::Utc::now(),
            event: AlienEvent::PushingResource {
                resource_name: function_name.to_string(),
                resource_type: "function".to_string(),
            },
            state: EventState::Started,
        };
        let _ = tx.send(ReleaseUiEvent::AlienEventChange(event_change));

        thread::sleep(Duration::from_millis(30));

        // Complete quickly
        let event_change = EventChange::StateChanged {
            id: format!("function-{}", i),
            new_state: EventState::Success,
            updated_at: chrono::Utc::now(),
        };
        let _ = tx.send(ReleaseUiEvent::AlienEventChange(event_change));
    }

    thread::sleep(Duration::from_millis(200));

    // Push stack complete
    let event_change = EventChange::StateChanged {
        id: "push-stack-1".to_string(),
        new_state: EventState::Success,
        updated_at: chrono::Utc::now(),
    };
    let _ = tx.send(ReleaseUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(200));

    // Creating release on platform
    let event_change = EventChange::Created {
        id: "create-release-1".to_string(),
        parent_id: None,
        created_at: chrono::Utc::now(),
        event: AlienEvent::CreatingRelease {
            project: "my-project".to_string(),
        },
        state: EventState::Started,
    };
    let _ = tx.send(ReleaseUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(300));

    // Creating release complete
    let event_change = EventChange::StateChanged {
        id: "create-release-1".to_string(),
        new_state: EventState::Success,
        updated_at: chrono::Utc::now(),
    };
    let _ = tx.send(ReleaseUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(100));

    // Send successful result
    let _ = tx.send(ReleaseUiEvent::ReleaseFinished(Ok(
        ReleaseResult::Success {
            release_id: "rel_abc123xyz789".to_string(),
        },
    )));

    thread::sleep(Duration::from_millis(2000));
}

// Helper functions

fn send_config_success(tx: &std::sync::mpsc::Sender<ReleaseUiEvent>) {
    // Configuration success
    let event_change = EventChange::Created {
        id: "config-1".to_string(),
        parent_id: None,
        created_at: chrono::Utc::now(),
        event: AlienEvent::LoadingConfiguration,
        state: EventState::Success,
    };
    let _ = tx.send(ReleaseUiEvent::AlienEventChange(event_change));
}

fn send_function_pushing(
    tx: &std::sync::mpsc::Sender<ReleaseUiEvent>,
    function_id: &str,
    function_name: &str,
    uploaded_mb: u64,
    total_mb: u64,
) {
    let event_change = EventChange::Created {
        id: format!("{}-push", function_id),
        parent_id: Some(function_id.to_string()),
        created_at: chrono::Utc::now(),
        event: AlienEvent::PushingImage {
            image: function_name.to_string(),
            progress: Some(PushProgress {
                operation: "uploading".to_string(),
                layers_uploaded: 4,
                total_layers: 4,
                bytes_uploaded: uploaded_mb * 1024 * 1024,
                total_bytes: total_mb * 1024 * 1024,
            }),
        },
        state: EventState::None,
    };
    let _ = tx.send(ReleaseUiEvent::AlienEventChange(event_change));
}
