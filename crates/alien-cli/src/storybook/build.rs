//! Build command simulation functions for the storybook

use super::{
    create_build_error, create_config_error, create_preflight_error, create_template_error,
};
use alien_cli::tui::{BuildUiComponent, BuildUiEvent, BuildUiProps};
use alien_core::{AlienEvent, EventChange, EventState};
use clap::Subcommand;
use std::{thread, time::Duration};

#[derive(Subcommand)]
pub enum BuildDemo {
    /// Loading configuration
    #[command(name = "1")]
    Config,
    /// Configuration error
    #[command(name = "1a")]
    ConfigError,
    /// Running preflights
    #[command(name = "2")]
    Preflights,
    /// Preflight validation failed
    #[command(name = "2a")]
    PreflightsFailed,
    /// Building functions
    #[command(name = "3")]
    Functions,
    /// Function build failed
    #[command(name = "3a")]
    FunctionsFailed,
    /// Build failed with fail-fast
    #[command(name = "3b")]
    FunctionsFailFast,
    /// Single function build
    #[command(name = "3c")]
    SingleFunction,
    /// Many functions build
    #[command(name = "3d")]
    ManyFunctions,
    /// Generating template
    #[command(name = "4")]
    Template,
    /// Template generation failed
    #[command(name = "4a")]
    TemplateFailed,
    /// Build success
    #[command(name = "5")]
    Success,
}

impl BuildDemo {
    pub fn run(self) -> color_eyre::Result<()> {
        let (platform, output_dir, simulate_fn): (
            &str,
            &str,
            fn(std::sync::mpsc::Sender<alien_cli::tui::BuildUiEvent>),
        ) = match self {
            Self::Config => ("aws", "./.alien", simulate_build_1),
            Self::ConfigError => ("aws", "./.alien", simulate_build_1a),
            Self::Preflights => ("aws", "./.alien", simulate_build_2),
            Self::PreflightsFailed => ("aws", "./.alien", simulate_build_2a),
            Self::Functions => ("aws", "./.alien", simulate_build_3),
            Self::FunctionsFailed => ("aws", "./.alien", simulate_build_3a),
            Self::FunctionsFailFast => ("aws", "./.alien", simulate_build_3b),
            Self::SingleFunction => ("aws", "./.alien", simulate_build_3c),
            Self::ManyFunctions => ("large-app", "./.alien", simulate_build_3d),
            Self::Template => ("aws", "./.alien", simulate_build_4),
            Self::TemplateFailed => ("aws", "./.alien", simulate_build_4a),
            Self::Success => ("aws", "./.alien", simulate_build_5),
        };

        run_build_demo_impl(platform, output_dir, simulate_fn)
    }
}

/// Run a build demo with the given platform and output directory
fn run_build_demo_impl(
    platform: &str,
    output_dir: &str,
    simulate_fn: fn(std::sync::mpsc::Sender<BuildUiEvent>),
) -> color_eyre::Result<()> {
    // Create the BuildUiComponent with props
    let props = BuildUiProps {
        platform: platform.to_string(),
        output_dir: output_dir.to_string(),
        on_result: None,
        on_cancel: None,
    };

    let mut ui_component = BuildUiComponent::new(props);

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

/// Simulate build-1: Loading configuration
pub fn simulate_build_1(tx: std::sync::mpsc::Sender<BuildUiEvent>) {
    thread::sleep(Duration::from_millis(100));

    // Send LoadingConfiguration started event
    let event_change = EventChange::Created {
        id: "config-1".to_string(),
        parent_id: None,
        event: AlienEvent::LoadingConfiguration,
        state: EventState::Started,
        created_at: chrono::Utc::now(),
    };
    let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));

    // Keep running for demo duration
    thread::sleep(Duration::from_millis(2000));
}

/// Simulate build-1a: Configuration loading failed
pub fn simulate_build_1a(tx: std::sync::mpsc::Sender<BuildUiEvent>) {
    thread::sleep(Duration::from_millis(100));

    // Send LoadingConfiguration started event
    let event_change = EventChange::Created {
        id: "config-1".to_string(),
        parent_id: None,
        event: AlienEvent::LoadingConfiguration,
        state: EventState::Started,
        created_at: chrono::Utc::now(),
    };
    let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(300));

    // Send configuration failed event
    let event_change = EventChange::StateChanged {
        id: "config-1".to_string(),
        new_state: EventState::Failed { error: None },
        updated_at: chrono::Utc::now(),
    };
    let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(100));

    // Send build failed result
    let config_error = create_config_error();
    let _ = tx.send(BuildUiEvent::BuildFinished(Err(config_error)));

    thread::sleep(Duration::from_millis(2000));
}

/// Simulate build-2: Running preflights after config loaded
pub fn simulate_build_2(tx: std::sync::mpsc::Sender<BuildUiEvent>) {
    thread::sleep(Duration::from_millis(100));

    // Configuration success
    let event_change = EventChange::Created {
        id: "config-1".to_string(),
        parent_id: None,
        event: AlienEvent::LoadingConfiguration,
        state: EventState::Success,
        created_at: chrono::Utc::now(),
    };
    let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(200));

    // Preflights started
    let event_change = EventChange::Created {
        id: "preflights-1".to_string(),
        parent_id: None,
        event: AlienEvent::RunningPreflights {
            stack: "my-app".to_string(),
            platform: "aws".to_string(),
        },
        state: EventState::Started,
        created_at: chrono::Utc::now(),
    };
    let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(2000));
}

/// Simulate build-2a: Preflight validation failed
pub fn simulate_build_2a(tx: std::sync::mpsc::Sender<BuildUiEvent>) {
    thread::sleep(Duration::from_millis(100));

    // Configuration success
    let event_change = EventChange::Created {
        id: "config-1".to_string(),
        parent_id: None,
        event: AlienEvent::LoadingConfiguration,
        state: EventState::Success,
        created_at: chrono::Utc::now(),
    };
    let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(200));

    // Preflights started
    let event_change = EventChange::Created {
        id: "preflights-1".to_string(),
        parent_id: None,
        event: AlienEvent::RunningPreflights {
            stack: "my-app".to_string(),
            platform: "aws".to_string(),
        },
        state: EventState::Started,
        created_at: chrono::Utc::now(),
    };
    let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(300));

    // Preflights failed
    let event_change = EventChange::StateChanged {
        id: "preflights-1".to_string(),
        new_state: EventState::Failed { error: None },
        updated_at: chrono::Utc::now(),
    };
    let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(100));

    // Send preflight error result
    let preflight_error = create_preflight_error();
    let _ = tx.send(BuildUiEvent::BuildFinished(Err(preflight_error)));

    thread::sleep(Duration::from_millis(2000));
}

/// Simulate build-3: Building functions (4 functions with mixed states)
pub fn simulate_build_3(tx: std::sync::mpsc::Sender<BuildUiEvent>) {
    thread::sleep(Duration::from_millis(100));

    // Configuration and preflights success
    send_config_and_preflights_success(&tx);

    thread::sleep(Duration::from_millis(200));

    // Start building 4 functions with different states
    let functions = [
        "api-handler",
        "background-worker",
        "data-processor",
        "notification-service",
    ];

    for (i, function_name) in functions.iter().enumerate() {
        let event_change = EventChange::Created {
            id: format!("function-{}", i),
            parent_id: None,
            created_at: chrono::Utc::now(),
            event: AlienEvent::BuildingResource {
                resource_name: function_name.to_string(),
                resource_type: "function".to_string(),
                related_resources: vec![],
            },
            state: EventState::Started,
        };
        let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));

        thread::sleep(Duration::from_millis(100));

        // Show different phases for different functions
        match i {
            0 | 2 => {
                // Some functions: building
                let event_change = EventChange::Created {
                    id: format!("build-{}", i),
                    parent_id: Some(format!("function-{}", i)),
                    created_at: chrono::Utc::now(),
                    event: AlienEvent::BuildingImage {
                        image: function_name.to_string(),
                    },
                    state: EventState::None,
                };
                let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));
            }
            1 | 3 => {
                // Others: compiling
                let event_change = EventChange::Created {
                    id: format!("compile-{}", i),
                    parent_id: Some(format!("function-{}", i)),
                    created_at: chrono::Utc::now(),
                    event: AlienEvent::CompilingCode {
                        language: "rust".to_string(),
                        progress: Some(format!("Compiling {} v0.1.0", function_name)),
                    },
                    state: EventState::None,
                };
                let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));
            }
            _ => {}
        }
    }

    thread::sleep(Duration::from_millis(2000));
}

/// Simulate build-3a: Single function build failure
pub fn simulate_build_3a(tx: std::sync::mpsc::Sender<BuildUiEvent>) {
    thread::sleep(Duration::from_millis(100));

    // Configuration and preflights success
    send_config_and_preflights_success(&tx);

    thread::sleep(Duration::from_millis(200));

    // Start building single function
    let event_change = EventChange::Created {
        id: "function-1".to_string(),
        parent_id: None,
        created_at: chrono::Utc::now(),
        event: AlienEvent::BuildingResource {
            resource_name: "data-analyzer".to_string(),
            resource_type: "function".to_string(),
            related_resources: vec![],
        },
        state: EventState::Started,
    };
    let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(200));

    // Show compiling
    let event_change = EventChange::Created {
        id: "compile-1".to_string(),
        parent_id: Some("function-1".to_string()),
        created_at: chrono::Utc::now(),
        event: AlienEvent::CompilingCode {
            language: "rust".to_string(),
            progress: Some("Compiling data-analyzer v0.1.0".to_string()),
        },
        state: EventState::None,
    };
    let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(300));

    // Function fails
    let event_change = EventChange::StateChanged {
        id: "function-1".to_string(),
        new_state: EventState::Failed { error: None },
        updated_at: chrono::Utc::now(),
    };
    let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(100));

    // Send build error result
    let build_error = create_build_error();
    let _ = tx.send(BuildUiEvent::BuildFinished(Err(build_error)));

    thread::sleep(Duration::from_millis(2000));
}

/// Simulate build-3b: Multiple functions with fail-fast cancellation
pub fn simulate_build_3b(tx: std::sync::mpsc::Sender<BuildUiEvent>) {
    thread::sleep(Duration::from_millis(100));

    // Configuration and preflights success
    send_config_and_preflights_success(&tx);

    thread::sleep(Duration::from_millis(200));

    // Start building 5 functions
    let functions = [
        "auth-service",
        "payment-processor",
        "data-analyzer",
        "report-generator",
        "notification-service",
    ];

    for (i, function_name) in functions.iter().enumerate() {
        let event_change = EventChange::Created {
            id: format!("function-{}", i),
            parent_id: None,
            created_at: chrono::Utc::now(),
            event: AlienEvent::BuildingResource {
                resource_name: function_name.to_string(),
                resource_type: "function".to_string(),
                related_resources: vec![],
            },
            state: EventState::Started,
        };
        let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));

        thread::sleep(Duration::from_millis(50));

        match i {
            0 => {
                // auth-service: completed
                let event_change = EventChange::StateChanged {
                    id: format!("function-{}", i),
                    new_state: EventState::Success,
                    updated_at: chrono::Utc::now(),
                };
                let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));
            }
            1 => {
                // payment-processor: compiling
                let event_change = EventChange::Created {
                    id: format!("compile-{}", i),
                    parent_id: Some(format!("function-{}", i)),
                    created_at: chrono::Utc::now(),
                    event: AlienEvent::CompilingCode {
                        language: "rust".to_string(),
                        progress: Some("Compiling payment-processor v0.1.0".to_string()),
                    },
                    state: EventState::None,
                };
                let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));
            }
            2 => {
                // data-analyzer: compiling (will fail)
                let event_change = EventChange::Created {
                    id: format!("compile-{}", i),
                    parent_id: Some(format!("function-{}", i)),
                    created_at: chrono::Utc::now(),
                    event: AlienEvent::CompilingCode {
                        language: "rust".to_string(),
                        progress: Some("Compiling data-analyzer v0.1.0".to_string()),
                    },
                    state: EventState::None,
                };
                let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));
            }
            _ => {
                // Others: queued (will be cancelled)
            }
        }
    }

    thread::sleep(Duration::from_millis(300));

    // data-analyzer fails
    let event_change = EventChange::StateChanged {
        id: "function-2".to_string(),
        new_state: EventState::Failed { error: None },
        updated_at: chrono::Utc::now(),
    };
    let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(100));

    // Send build error result
    let build_error = create_build_error();
    let _ = tx.send(BuildUiEvent::BuildFinished(Err(build_error)));

    thread::sleep(Duration::from_millis(2000));
}

/// Simulate build-3c: Building functions (1 function - simple case)
pub fn simulate_build_3c(tx: std::sync::mpsc::Sender<BuildUiEvent>) {
    thread::sleep(Duration::from_millis(100));

    // Configuration and preflights success
    send_config_and_preflights_success(&tx);

    thread::sleep(Duration::from_millis(200));

    // Start building single function
    let event_change = EventChange::Created {
        id: "function-1".to_string(),
        parent_id: None,
        created_at: chrono::Utc::now(),
        event: AlienEvent::BuildingResource {
            resource_name: "api-handler".to_string(),
            resource_type: "function".to_string(),
            related_resources: vec![],
        },
        state: EventState::Started,
    };
    let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(200));

    // Show compiling
    let event_change = EventChange::Created {
        id: "compile-1".to_string(),
        parent_id: Some("function-1".to_string()),
        created_at: chrono::Utc::now(),
        event: AlienEvent::CompilingCode {
            language: "rust".to_string(),
            progress: Some("Compiling api-handler v0.1.0".to_string()),
        },
        state: EventState::None,
    };
    let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(2000));
}

/// Simulate build-3d: Building functions (10 functions - complex case)
pub fn simulate_build_3d(tx: std::sync::mpsc::Sender<BuildUiEvent>) {
    thread::sleep(Duration::from_millis(100));

    // Configuration and preflights success
    send_config_and_preflights_success(&tx);

    thread::sleep(Duration::from_millis(200));

    // Start building 10 functions
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
            parent_id: None,
            created_at: chrono::Utc::now(),
            event: AlienEvent::BuildingResource {
                resource_name: function_name.to_string(),
                resource_type: "function".to_string(),
                related_resources: vec![],
            },
            state: EventState::Started,
        };
        let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));

        thread::sleep(Duration::from_millis(30));

        // Show different states for different functions
        match i {
            0..=2 => {
                // First 3: completed
                let event_change = EventChange::StateChanged {
                    id: format!("function-{}", i),
                    new_state: EventState::Success,
                    updated_at: chrono::Utc::now(),
                };
                let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));
            }
            3..=6 => {
                // Next 4: compiling
                let event_change = EventChange::Created {
                    id: format!("compile-{}", i),
                    parent_id: Some(format!("function-{}", i)),
                    created_at: chrono::Utc::now(),
                    event: AlienEvent::CompilingCode {
                        language: "rust".to_string(),
                        progress: Some(format!("Compiling {} v0.1.0", function_name)),
                    },
                    state: EventState::None,
                };
                let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));
            }
            _ => {
                // Last few: building image
                let event_change = EventChange::Created {
                    id: format!("build-{}", i),
                    parent_id: Some(format!("function-{}", i)),
                    created_at: chrono::Utc::now(),
                    event: AlienEvent::BuildingImage {
                        image: function_name.to_string(),
                    },
                    state: EventState::None,
                };
                let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));
            }
        }
    }

    thread::sleep(Duration::from_millis(2000));
}

/// Simulate build-4: Generating deployment template
pub fn simulate_build_4(tx: std::sync::mpsc::Sender<BuildUiEvent>) {
    thread::sleep(Duration::from_millis(100));

    // Configuration and preflights success
    send_config_and_preflights_success(&tx);

    thread::sleep(Duration::from_millis(200));

    // Build and complete all functions quickly using the proper flow
    let functions = [
        "api-handler",
        "background-worker",
        "data-processor",
        "notification-service",
    ];
    for (i, function_name) in functions.iter().enumerate() {
        // Start the function
        let event_change = EventChange::Created {
            id: format!("function-{}", i),
            parent_id: None,
            created_at: chrono::Utc::now(),
            event: AlienEvent::BuildingResource {
                resource_name: function_name.to_string(),
                resource_type: "function".to_string(),
                related_resources: vec![],
            },
            state: EventState::Started,
        };
        let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));
        thread::sleep(Duration::from_millis(30));

        // Complete the function quickly
        let event_change = EventChange::StateChanged {
            id: format!("function-{}", i),
            new_state: EventState::Success,
            updated_at: chrono::Utc::now(),
        };
        let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));
        thread::sleep(Duration::from_millis(30));
    }

    thread::sleep(Duration::from_millis(200));

    // Start template generation
    let event_change = EventChange::Created {
        id: "template-1".to_string(),
        parent_id: None,
        created_at: chrono::Utc::now(),
        event: AlienEvent::GeneratingTemplate {
            platform: "aws".to_string(),
        },
        state: EventState::Started,
    };
    let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(2000));
}

/// Simulate build-4a: Template generation failed
pub fn simulate_build_4a(tx: std::sync::mpsc::Sender<BuildUiEvent>) {
    thread::sleep(Duration::from_millis(100));

    // Configuration and preflights success
    send_config_and_preflights_success(&tx);

    thread::sleep(Duration::from_millis(200));

    // Build and complete functions quickly using the proper flow
    let functions = ["user-service", "notification-handler"];
    for (i, function_name) in functions.iter().enumerate() {
        // Start the function
        let event_change = EventChange::Created {
            id: format!("function-{}", i),
            parent_id: None,
            created_at: chrono::Utc::now(),
            event: AlienEvent::BuildingResource {
                resource_name: function_name.to_string(),
                resource_type: "function".to_string(),
                related_resources: vec![],
            },
            state: EventState::Started,
        };
        let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));
        thread::sleep(Duration::from_millis(30));

        // Complete the function quickly
        let event_change = EventChange::StateChanged {
            id: format!("function-{}", i),
            new_state: EventState::Success,
            updated_at: chrono::Utc::now(),
        };
        let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));
        thread::sleep(Duration::from_millis(30));
    }

    thread::sleep(Duration::from_millis(200));

    // Start template generation
    let event_change = EventChange::Created {
        id: "template-1".to_string(),
        parent_id: None,
        created_at: chrono::Utc::now(),
        event: AlienEvent::GeneratingTemplate {
            platform: "aws".to_string(),
        },
        state: EventState::Started,
    };
    let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(300));

    // Template generation fails
    let event_change = EventChange::StateChanged {
        id: "template-1".to_string(),
        new_state: EventState::Failed { error: None },
        updated_at: chrono::Utc::now(),
    };
    let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(100));

    // Send template error result
    let template_error = create_template_error();
    let _ = tx.send(BuildUiEvent::BuildFinished(Err(template_error)));

    thread::sleep(Duration::from_millis(2000));
}

/// Simulate build-5: Build completed successfully
pub fn simulate_build_5(tx: std::sync::mpsc::Sender<BuildUiEvent>) {
    thread::sleep(Duration::from_millis(100));

    // Configuration and preflights success
    send_config_and_preflights_success(&tx);

    thread::sleep(Duration::from_millis(200));

    // Build and complete all functions using the proper flow
    let functions = [
        "api-handler",
        "background-worker",
        "data-processor",
        "notification-service",
    ];
    for (i, function_name) in functions.iter().enumerate() {
        // Start the function
        let event_change = EventChange::Created {
            id: format!("function-{}", i),
            parent_id: None,
            created_at: chrono::Utc::now(),
            event: AlienEvent::BuildingResource {
                resource_name: function_name.to_string(),
                resource_type: "function".to_string(),
                related_resources: vec![],
            },
            state: EventState::Started,
        };
        let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));
        thread::sleep(Duration::from_millis(30));

        // Complete the function quickly
        let event_change = EventChange::StateChanged {
            id: format!("function-{}", i),
            new_state: EventState::Success,
            updated_at: chrono::Utc::now(),
        };
        let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));
        thread::sleep(Duration::from_millis(30));
    }

    thread::sleep(Duration::from_millis(200));

    // Template generation - start and complete
    let event_change = EventChange::Created {
        id: "template-1".to_string(),
        parent_id: None,
        created_at: chrono::Utc::now(),
        event: AlienEvent::GeneratingTemplate {
            platform: "aws".to_string(),
        },
        state: EventState::Started,
    };
    let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(100));

    // Complete template generation
    let event_change = EventChange::StateChanged {
        id: "template-1".to_string(),
        new_state: EventState::Success,
        updated_at: chrono::Utc::now(),
    };
    let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(200));

    // Send successful result
    let _ = tx.send(BuildUiEvent::BuildFinished(Ok(
        alien_cli::tui::BuildResult::Success,
    )));

    thread::sleep(Duration::from_millis(2000));
}

// Helper functions

fn send_config_and_preflights_success(tx: &std::sync::mpsc::Sender<BuildUiEvent>) {
    // Configuration success
    let event_change = EventChange::Created {
        id: "config-1".to_string(),
        parent_id: None,
        created_at: chrono::Utc::now(),
        event: AlienEvent::LoadingConfiguration,
        state: EventState::Success,
    };
    let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));

    thread::sleep(Duration::from_millis(100));

    // Preflights success
    let event_change = EventChange::Created {
        id: "preflights-1".to_string(),
        parent_id: None,
        created_at: chrono::Utc::now(),
        event: AlienEvent::RunningPreflights {
            stack: "my-app".to_string(),
            platform: "aws".to_string(),
        },
        state: EventState::Success,
    };
    let _ = tx.send(BuildUiEvent::AlienEventChange(event_change));
}
