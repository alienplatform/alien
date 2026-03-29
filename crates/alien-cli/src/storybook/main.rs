//! Alien CLI TUI Storybook
//!
//! A testing tool for TUI components. This allows developers to test and iterate
//! on UI components without running the full alien CLI commands.
//!
//! Usage: cargo run --bin alien-cli-storybook <demo-name>

use alien_core::{Resource, ResourceStatus, StackResourceState};
use alien_error::{AlienError, ContextError};
use clap::{Parser, Subcommand};
use color_eyre::Result;

mod build;
mod dev;
mod release;
pub mod tui;

#[derive(Parser)]
#[command(name = "alien-cli-storybook")]
#[command(about = "A testing tool for TUI components")]
#[command(
    long_about = "A testing tool for TUI components that allows developers to test and iterate on UI components without running the full alien CLI commands.

Examples:
  alien-cli-storybook build 1        # Run build demo 1
  alien-cli-storybook build 1a       # Run build demo 1a
  alien-cli-storybook release 1      # Run release demo 1
  alien-cli-storybook release 4      # Run release success demo
  alien-cli-storybook dev 1          # Run dev fresh deploy demo
  alien-cli-storybook dev running    # Run dev with live logs

You can use spaces instead of hyphens: '1 a' instead of '1a'"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build command demos
    Build {
        #[command(subcommand)]
        demo: crate::build::BuildDemo,
    },
    /// Dev command demos
    Dev {
        #[command(subcommand)]
        demo: crate::dev::DevDemo,
    },
    /// Release command demos
    Release {
        #[command(subcommand)]
        demo: crate::release::ReleaseDemo,
    },
    /// TUI view demos (deployments list, deployment detail, etc.)
    Tui {
        #[command(subcommand)]
        view: TuiView,
    },
}

#[derive(Subcommand)]
enum TuiView {
    /// Deployments list view demos
    Deployments {
        #[command(subcommand)]
        demo: crate::tui::demos::DeploymentsListDemo,
    },
    /// Deployment detail view demos
    Detail {
        #[command(subcommand)]
        demo: crate::tui::demos::DeploymentDetailDemo,
    },
    /// Deployment groups view demos
    Dg {
        #[command(subcommand)]
        demo: crate::tui::demos::DeploymentGroupsDemo,
    },
    /// Commands view demos
    Commands {
        #[command(subcommand)]
        demo: crate::tui::demos::CommandsDemo,
    },
    /// Header widget demos
    Header {
        #[command(subcommand)]
        demo: crate::tui::demos::HeaderDemo,
    },
    /// Releases view demos
    Releases {
        #[command(subcommand)]
        demo: crate::tui::demos::ReleasesDemo,
    },
    /// Packages view demos
    Packages {
        #[command(subcommand)]
        demo: crate::tui::demos::PackagesDemo,
    },
    /// Error dialog demos
    Error {
        #[command(subcommand)]
        demo: crate::tui::demos::ErrorDialogDemo,
    },
    /// Logs view demos
    Logs {
        #[command(subcommand)]
        demo: crate::tui::demos::LogsViewDemo,
    },
    /// Search overlay demos
    Search {
        #[command(subcommand)]
        demo: crate::tui::demos::SearchDemo,
    },
    /// Tabs navigation demos
    Tabs {
        #[command(subcommand)]
        demo: crate::tui::demos::TabsDemo,
    },
    /// Deploy dialog demos
    Deploy {
        #[command(subcommand)]
        demo: crate::tui::demos::DeployDialogDemo,
    },
}

fn main() -> Result<()> {
    color_eyre::install()?;

    let cli = Cli::parse();

    match cli.command {
        Commands::Build { demo } => demo.run()?,
        Commands::Dev { demo } => demo.run()?,
        Commands::Release { demo } => demo.run()?,
        Commands::Tui { view } => match view {
            TuiView::Deployments { demo } => demo.run()?,
            TuiView::Detail { demo } => demo.run()?,
            TuiView::Dg { demo } => demo.run()?,
            TuiView::Commands { demo } => demo.run()?,
            TuiView::Header { demo } => demo.run()?,
            TuiView::Releases { demo } => demo.run()?,
            TuiView::Packages { demo } => demo.run()?,
            TuiView::Error { demo } => demo.run()?,
            TuiView::Logs { demo } => demo.run()?,
            TuiView::Search { demo } => demo.run()?,
            TuiView::Tabs { demo } => demo.run()?,
            TuiView::Deploy { demo } => demo.run()?,
        },
    }

    Ok(())
}

// Helper function to create mock resource states for storybook demos using real Alien resource types
pub fn create_mock_resource_state(
    alien_resource_type: &str,
    status: ResourceStatus,
) -> StackResourceState {
    use alien_core::{Function, FunctionCode, Storage, ToolchainConfig, Vault};

    // Create different Alien resource configs based on the type
    let config = match alien_resource_type {
        "vault" => {
            let vault = Vault::new("demo-vault".to_string()).build();
            Resource::new(vault)
        }
        "storage" => {
            let storage = Storage::new("demo-storage".to_string()).build();
            Resource::new(storage)
        }
        "function" => {
            let function = Function::new("demo-function".to_string())
                .permissions("default".to_string())
                .code(FunctionCode::Source {
                    src: "./src".to_string(),
                    toolchain: ToolchainConfig::TypeScript {
                        binary_name: Some("demo".to_string()),
                    },
                })
                .build();
            Resource::new(function)
        }
        _ => {
            // Default to function for unknown types
            let function = Function::new("demo-function".to_string())
                .permissions("default".to_string())
                .code(FunctionCode::Source {
                    src: "./src".to_string(),
                    toolchain: ToolchainConfig::TypeScript {
                        binary_name: Some("demo".to_string()),
                    },
                })
                .build();
            Resource::new(function)
        }
    };

    StackResourceState {
        resource_type: alien_resource_type.to_string(),
        internal_state: None,
        status,
        outputs: None,
        config,
        previous_config: None,
        retry_attempt: 0,
        error: None,
        is_externally_provisioned: false,
        lifecycle: None,
        dependencies: vec![],
        last_failed_state: None,
        remote_binding_params: None,
    }
}

// Error creators
pub fn create_config_error() -> AlienError<alien_cli::error::ErrorData> {
    use alien_cli::error::ErrorData;

    // Create a realistic configuration error similar to the actual codebase
    // Based on config.rs: "Could not find alien.ts, alien.js, or alien.json"
    AlienError::new(ErrorData::ConfigurationError {
        message: "Could not find alien.ts, alien.js, or alien.json in /Users/dev/my-app"
            .to_string(),
    })
}

pub fn create_preflight_error() -> AlienError<alien_cli::error::ErrorData> {
    use alien_cli::error::ErrorData;
    use alien_preflights::{error::ErrorData as PreflightErrorData, CheckResult};

    // Create realistic preflight validation results using proper CheckResult structures
    let results = vec![
        CheckResult::success().with_check_description("Stack should contain only allowed user-defined resources".to_string()),
        CheckResult::success().with_check_description("Resources that must appear at most once shouldn't have multiple instances".to_string()),
        CheckResult::failed(vec![
            "Function 'api-handler' has 2 queue triggers, but only one queue trigger per function is supported".to_string(),
            "Function 'worker-service' has 3 queue triggers, but only one queue trigger per function is supported".to_string(),
        ]).with_check_description("Functions should have at most one queue trigger each".to_string()),
        CheckResult::failed(vec![
            "Function 'notification-service' references unknown storage 'user-uploads'".to_string(),
        ]).with_check_description("All resource references should point to resources that exist within the stack".to_string()),
        CheckResult::with_warnings(vec![
            "Resource 'api-handler' has a long dependency chain (5 levels deep)".to_string(),
        ]).with_check_description("User-defined resource dependencies should be valid and shouldn't create circular references".to_string()),
    ];

    // Simulate the real error chain that happens in alien-build:
    // 1. PreflightErrorData::ValidationFailed (the actual preflight validation error)
    // 2. alien_build::ErrorData::StackProcessorFailed (from alien-build)
    // 3. alien_cli::ErrorData::BuildFailed (from alien-cli's build.rs)

    let preflight_error = alien_error::AlienError::new(PreflightErrorData::ValidationFailed {
        error_count: 2,
        warning_count: 1,
        results: results,
    });

    // This simulates the context that alien-build adds (from lib.rs line 83-85)
    let build_error =
        preflight_error.context(alien_build::error::ErrorData::StackProcessorFailed {
            message: "Failed to run build-time preflights".to_string(),
        });

    // This simulates the context that alien-cli adds (from build.rs line 282)
    build_error.context(ErrorData::BuildFailed)
}

pub fn create_build_error() -> AlienError<alien_cli::error::ErrorData> {
    use alien_cli::error::ErrorData;

    let compilation_output = create_rust_compilation_error();

    // Create realistic error exactly like alien-build/src/toolchain/rust.rs:314-318
    // This simulates when cargo zigbuild fails during Rust compilation
    let build_error =
        alien_error::AlienError::new(alien_build::error::ErrorData::ImageBuildFailed {
            function_name: "data-analyzer".to_string(),
            reason: "Cargo zigbuild failed".to_string(),
            build_output: Some(compilation_output),
        });

    // Convert from alien_build::error::ErrorData to alien_cli::error::ErrorData
    // The CLI layer wraps build errors with BuildFailed for user display
    build_error.context(ErrorData::BuildFailed)
}

pub fn create_template_error() -> AlienError<alien_cli::error::ErrorData> {
    use alien_cli::error::ErrorData;

    // Create a realistic template generation error based on actual codebase patterns
    // Similar to CloudFormation resource generation failures
    AlienError::new(ErrorData::ConfigurationError {
        message: "Failed to generate deployment template".to_string(),
    })
    .context(ErrorData::BuildFailed)
}

pub fn create_rust_compilation_error() -> String {
    vec![
        "cargo build failed with multiple errors and warnings:",
        "",
        "error[E0599]: no method named `to_report` found for struct `DataAnalysis` in the current scope",
        " --> src/report_generator.rs:45:23",
        "  |",
        "45 |         let report = analysis.to_report();",
        "  |                       ^^^^^^^^^ method not found in `DataAnalysis`",
        "  |",
        "  = help: items from traits can only be used if the trait is in scope",
        "help: the following trait defines an item `to_report`, perhaps you need to import it:",
        "  |",
        "1  | use crate::reporting::ToReport;",
        "  |",
        "",
        "error[E0308]: mismatched types",
        " --> src/report_generator.rs:52:13",
        "  |",
        "52 |             Ok(report_data)",
        "  |             ^^^^^^^^^^^^^^^^ expected `Result<ReportOutput, ReportError>`, found `Result<ReportData, _>`",
        "  |",
        "  = note: expected enum `Result<ReportOutput, ReportError>`",
        "             found enum `Result<ReportData, DatabaseError>`",
        "",
        "error[E0277]: the trait bound `ReportData: Serialize` is not satisfied",
        " --> src/report_generator.rs:78:23",
        "  |",
        "78 |         serde_json::to_string(&report_data)?",
        "  |         ^^^^^^^^^^^^^^^^^^^^^ the trait `Serialize` is not implemented for `ReportData`",
        "  |",
        "  = help: the following other types implement trait `Serialize`:",
        "            bool",
        "            char",
        "            isize",
        "            i8",
        "          and 127 others",
        "  = note: required for `&ReportData` to implement `Serialize`",
        "  = note: required by a bound in `serde_json::to_string`",
        "",
        "error[E0412]: cannot find type `ChartConfig` in this scope",
        " --> src/report_generator.rs:105:25",
        "  |",
        "105 |     fn generate_chart(config: &ChartConfig) -> Result<Chart, ChartError> {",
        "  |                         ^^^^^^^^^^^ not found in this scope",
        "help: consider importing this struct:",
        "  |",
        "1   | use crate::charts::ChartConfig;",
        "  |",
        "",
        "error[E0425]: cannot find function `validate_data` in this scope",
        " --> src/report_generator.rs:112:17",
        "  |",
        "112 |         if !validate_data(&raw_data) {",
        "  |                 ^^^^^^^^^^^^^ not found in this scope",
        "help: consider importing this function:",
        "  |",
        "1   | use crate::validation::validate_data;",
        "  |",
        "",
        "warning: unused import: `std::collections::HashMap`",
        " --> src/report_generator.rs:3:5",
        "  |",
        "3 | use std::collections::HashMap;",
        "  |     ^^^^^^^^^^^^^^^^^^^^^^^^^",
        "  |",
        "  = note: `#[warn(unused_imports)]` on by default",
        "",
        "error[E0596]: cannot borrow `*self` as mutable",
        " --> src/report_generator.rs:89:9",
        "  |",
        "89 |         self.cache.insert(key, value);",
        "  |         ^^^^^ `self` is a `&` reference, so the data it refers to cannot be borrowed as mutable",
        "  |",
        "  = help: consider changing this to be a mutable reference:",
        "  |",
        "12 |     fn update_cache(&mut self, key: String, value: CacheValue) {",
        "  |                     ~~~~~~~~~",
        "",
        "error[E0515]: cannot return reference to temporary value",
        " --> src/report_generator.rs:134:5",
        "  |",
        "134 |     &format!(\"Report generated at {}\", timestamp)",
        "  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^",
        "  |     |",
        "  |     returns a reference to data owned by the current function",
        "  |     temporary value created here",
        "",
        "error: aborting due to 7 previous errors; 1 warning emitted",
        "",
        "Some errors have detailed explanations: E0277, E0308, E0412, E0425, E0515, E0596, E0599.",
        "For more information about an error, try `rustc --explain E0277`.",
        "",
        "Build failed in Docker container. See above for compilation errors.",
        "To fix these issues:",
        "1. Add missing imports for ToReport, ChartConfig, and validate_data",
        "2. Implement Serialize trait for ReportData",
        "3. Fix type mismatches between ReportOutput and ReportData",
        "4. Make update_cache method take &mut self",
        "5. Fix lifetime issues in format functions",
        "",
        "Compilation terminated with exit code 101"
    ].join("\n")
}

// Deployment-specific error creation functions for realistic demos

pub fn create_deployment_target_error() -> AlienError<alien_cli::error::ErrorData> {
    use alien_cli::error::ErrorData;

    // Simulate AWS credential configuration issues that occur during apply command
    // This emulates errors from deployment_target.rs when resolving AWS credentials
    let aws_auth_error = alien_error::AlienError::new(alien_infra::ErrorData::AuthenticationFailed {
        message: "No credentials found. Tried: environment variables (AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY), AWS shared credentials file (~/.aws/credentials), IAM instance profile, and ECS task role".to_string(),
        method: Some("aws-cli".to_string()),
    });

    // Context from DeploymentTargetResolver (deployment_target.rs:156)
    let resolver_error = aws_auth_error.context(alien_infra::ErrorData::DeploymentTargetInvalid {
        message: "Failed to resolve AWS deployment target".to_string(),
        field_name: Some("credentials".to_string()),
    });

    // Final context from apply.rs where this gets caught
    resolver_error.context(ErrorData::ConfigurationError {
        message: "Failed to determine deployment target from configuration".to_string(),
    })
}

pub fn create_deployment_runtime_preflight_error() -> AlienError<alien_cli::error::ErrorData> {
    use alien_cli::error::ErrorData;
    use alien_preflights::{error::ErrorData as PreflightErrorData, CheckResult};

    // Create realistic runtime preflight validation results - checks that fail during apply
    let results = vec![
        CheckResult::success().with_check_description("Verifying AWS account permissions".to_string()),
        CheckResult::success().with_check_description("Checking IAM role permissions for Lambda deployment".to_string()),
        CheckResult::failed(vec![
            "IAM role 'alien-demo-default-sa' does not exist in target AWS account (123456789012)".to_string(),
            "Required IAM role policy 'AlienLambdaExecutionRole' is missing or has insufficient permissions".to_string(),
        ]).with_check_description("Verifying required IAM roles and policies exist in target account".to_string()),
        CheckResult::failed(vec![
            "VPC endpoint for Lambda not accessible from subnet subnet-abc123def456".to_string(),
            "Security group sg-987654321fed does not allow HTTPS outbound traffic for ECR image pulls".to_string(),
        ]).with_check_description("Checking network connectivity for Lambda deployment".to_string()),
        CheckResult::with_warnings(vec![
            "Lambda function count (47) is approaching service quota limit (50) in region us-east-1".to_string(),
        ]).with_check_description("Validating AWS service quotas and limits".to_string()),
    ];

    // Simulate the error chain from runtime preflights during apply
    let preflight_error = alien_error::AlienError::new(PreflightErrorData::ValidationFailed {
        error_count: 2,
        warning_count: 1,
        results: results,
    });

    // Context from apply.rs when runtime preflights fail (apply.rs:304)
    preflight_error.context(ErrorData::ValidationError {
        field: "runtime-preflights".to_string(),
        message: "Runtime preflight checks failed during deployment".to_string(),
    })
}

pub fn create_deployment_planning_error() -> AlienError<alien_cli::error::ErrorData> {
    use alien_cli::error::ErrorData;

    // Simulate dependency resolution error during planning phase
    // This represents errors from StackExecutor::plan() in executor.rs
    let dependency_error =
        alien_error::AlienError::new(alien_infra::ErrorData::DependencyNotReady {
            resource_id: "api-handler".to_string(),
            dependency_id: "user-storage".to_string(),
        });

    // Context from executor.rs:376 when dependencies aren't met during planning
    let executor_error = dependency_error.context(alien_infra::ErrorData::ExecutionStepFailed {
        message: "Failed to generate deployment plan due to unresolved dependencies".to_string(),
        resource_id: Some("api-handler".to_string()),
    });

    // Final context from apply.rs where planning happens
    executor_error.context(ErrorData::ConfigurationError {
        message: "Deployment planning failed".to_string(),
    })
}

pub fn create_resource_deployment_error() -> AlienError<alien_cli::error::ErrorData> {
    use alien_cli::error::ErrorData;

    // Simulate AWS Lambda creation failure with realistic AWS error
    // This represents errors from function/aws.rs during resource provisioning
    let aws_error = alien_error::AlienError::new(alien_infra::ErrorData::CloudPlatformError {
        message: "User is not authorized to perform lambda:CreateFunction on resource: arn:aws:lambda:us-east-1:123456789012:function:demo-api-handler with an explicit deny in IAM policy 'RestrictLambdaAccess'".to_string(),
        resource_id: Some("api-handler".to_string()),
    });

    // Context from AwsFunctionController::create_start() (function/aws.rs:134)
    let function_error = aws_error.context(alien_infra::ErrorData::CloudPlatformError {
        message: "Failed to create Lambda function: User is not authorized to perform lambda:CreateFunction on resource: arn:aws:lambda:us-east-1:123456789012:function:demo-api-handler with an explicit deny in IAM policy 'RestrictLambdaAccess'".to_string(),
        resource_id: Some("api-handler".to_string()),
    });

    // Context from StackExecutor::step() when a resource step fails (executor.rs:1135)
    let executor_error = function_error.context(alien_infra::ErrorData::ExecutionStepFailed {
        message: "Resource provisioning failed after 3 retry attempts".to_string(),
        resource_id: Some("api-handler".to_string()),
    });

    // Final context from apply.rs where deployment execution happens
    executor_error.context(ErrorData::ApiRequestFailed {
        message: "Resource deployment failed".to_string(),
        url: None,
    })
}

pub fn create_resource_deletion_error() -> AlienError<alien_cli::error::ErrorData> {
    use alien_cli::error::ErrorData;

    // Simulate AWS Lambda deletion failure with dependency conflict
    // This represents errors from function/aws.rs during resource deletion
    let aws_error = alien_error::AlienError::new(alien_infra::ErrorData::CloudPlatformError {
        message: "Cannot delete function 'demo-api-handler' because it has a dependency: Event source mapping from SQS queue 'demo-user-notifications'".to_string(),
        resource_id: Some("api-handler".to_string()),
    });

    // Context from AwsFunctionController::delete_start() (function/aws.rs:874)
    let function_error = aws_error.context(alien_infra::ErrorData::CloudPlatformError {
        message: "Failed to delete Lambda function: InvalidParameterValueException - The function cannot be deleted due to existing dependencies. Please remove event source mappings before deleting the function".to_string(),
        resource_id: Some("api-handler".to_string()),
    });

    // Context from StackExecutor::step() when a resource deletion step fails (executor.rs:805)
    let executor_error = function_error.context(alien_infra::ErrorData::ExecutionStepFailed {
        message: "Resource deletion failed after 3 retry attempts".to_string(),
        resource_id: Some("api-handler".to_string()),
    });

    // Final context from destroy.rs where destruction execution happens
    executor_error.context(ErrorData::ApiRequestFailed {
        message: "Resource destruction failed".to_string(),
        url: None,
    })
}
