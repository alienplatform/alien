//! CLI commands for operations plugins.
//!
//! Operations plugins package named operations (`plugin/operation`) that run
//! inside a deployment via the commands interface. `init` scaffolds a new
//! plugin from the public SDK; `check` validates its manifest offline;
//! `test` runs its own test suite; `permissions` compiles its declared
//! permission requirements into a cloud-specific policy document; `publish`
//! uploads a custom plugin bundle (a ZIP with `metadata.json` + per-arch
//! binaries) to the platform so a workspace can use its operations; `list`
//! shows the catalog; `invoke` runs an operation and waits for its result.
//!
//! `package` builds a plugin's release binary and zips it with
//! `metadata.json` into the bundle format `publish` expects.
//!
//! `init`, `check`, `test`, and `package` are fully local and need no
//! platform account. `permissions`, `publish`, `list`, and `invoke` talk to
//! the Alien platform API and are only available with the `platform`
//! feature enabled.

#[cfg(feature = "platform")]
use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::error::Result;
use crate::execution_context::ExecutionMode;

mod check;
mod docs;
mod init;
mod package;
mod permissions;
#[cfg(feature = "platform")]
mod platform_actions;
mod test;

pub use check::check_task;
pub use docs::docs_task;
pub use init::init_task;
pub use package::package_task;
pub use permissions::{permissions_task, Cloud};
#[cfg(feature = "platform")]
pub use platform_actions::{invoke_task, list_task, publish_task};
#[cfg(feature = "platform")]
use platform_actions::InvokeTaskOptions;
pub use test::test_task;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Build, test, and manage operations plugins",
    long_about = "Build, test, and manage operations plugins.

Operations plugins package named operations you can run inside a deployment via
the commands interface (`plugin/operation`). `init`, `check`, `test`, and
`package` work fully offline against the public SDK; `permissions`, `publish`,
`list`, and `invoke` need a linked platform workspace.

EXAMPLES:
    # Scaffold a new plugin
    alien operations init my-plugin

    # Validate its manifest offline
    alien operations check

    # Run its own test suite
    alien operations test

    # See what AWS IAM permissions it needs
    alien operations permissions --cloud aws

    # Build a release binary and zip it into a bundle
    alien operations package

    # Publish a custom plugin bundle
    alien operations publish ./postgres-operations-1.0.0.zip

    # List available plugins (builtin + custom)
    alien operations list

    # Invoke an enabled operation without an AI agent
    alien operations invoke --deployment mycustomer/prod \\
      --operation kubernetes/get-pods \\
      --params '{\"namespace\": \"default\", \"maxResults\": 10}'
"
)]
pub struct OperationsArgs {
    #[command(subcommand)]
    pub action: OperationsAction,

    /// Project ID or name. Defaults to the linked project. Only used by
    /// actions that talk to the platform.
    #[arg(long, global = true)]
    pub project: Option<String>,

    /// Emit machine-readable JSON.
    #[arg(long, global = true)]
    pub json: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub enum OperationsAction {
    /// Scaffold a new operations plugin from the public SDK. Fully offline.
    Init {
        /// Plugin name (kebab-case), also used as the crate name.
        name: String,

        /// Destination directory. Defaults to `./<name>`.
        directory: Option<String>,
    },
    /// Validate a plugin's manifest offline. Fully offline.
    Check {
        /// Plugin directory containing `metadata.json`. Defaults to the
        /// current directory.
        directory: Option<String>,
    },
    /// Run a plugin's own test suite (`cargo test` in its directory).
    /// Fully offline.
    Test {
        /// Plugin directory. Defaults to the current directory.
        directory: Option<String>,
    },
    /// Compile a plugin's declared permission requirements into a
    /// cloud-specific policy document. Fully offline.
    Permissions {
        /// Plugin directory containing `metadata.json`. Defaults to the
        /// current directory.
        directory: Option<String>,

        /// Cloud to generate a policy for.
        #[arg(long)]
        cloud: Cloud,
    },
    /// Generate MCP tool schemas and a Markdown reference page from a
    /// plugin's manifest. Fully offline.
    Docs {
        /// Plugin directory containing `metadata.json`. Defaults to the
        /// current directory.
        directory: Option<String>,
    },
    /// Build a plugin's release binary and zip it with its manifest into a
    /// bundle ready for `publish`. Builds only for this host's architecture
    /// — publishing both amd64 and arm64 needs a build on each. Fully
    /// offline (aside from `cargo build`'s own dependency resolution).
    Package {
        /// Plugin directory. Defaults to the current directory.
        directory: Option<String>,
    },
    /// Publish a custom operations plugin bundle (ZIP) to your workspace.
    #[cfg(feature = "platform")]
    Publish {
        /// Path to the plugin bundle ZIP (contains metadata.json + binaries).
        bundle: PathBuf,
    },
    /// List available operations plugins (builtin + custom).
    #[cfg(feature = "platform")]
    List,
    /// Invoke an enabled operation and wait for its result.
    #[cfg(feature = "platform")]
    Invoke {
        /// Deployment ID, or <deployment-group-name>/<deployment-name>.
        #[arg(long)]
        deployment: String,

        /// Operation name in <plugin>/<operation> form.
        #[arg(long)]
        operation: String,

        /// Operation parameters as JSON.
        #[arg(long, default_value = "{}")]
        params: String,

        /// Timeout in seconds.
        #[arg(long, default_value = "60")]
        timeout: u64,

        /// If the operation requires approval, automatically create an access
        /// request instead of printing instructions.
        #[arg(long = "request-access")]
        request_access: bool,

        /// Approval duration to request with --request-access, e.g. 1h, 30m.
        #[arg(long = "access-duration", default_value = "1h")]
        access_duration: String,
    },
}

/// Dispatch `args` if its action is fully local (needs no platform account,
/// no manager URL, and no execution context of any kind) — `init`, `check`,
/// `test`, `permissions`, `docs`, `package`. Returns `None` for a
/// platform-only action (`publish`/`list`/`invoke`), so the caller knows to
/// fall through to context resolution instead.
///
/// Call this BEFORE resolving an [`ExecutionMode`] — a build without the
/// `platform` feature and no `ALIEN_MANAGER_URL` set fails context
/// resolution outright, which would otherwise make every local action
/// unusable even though none of them need a manager or platform account.
pub async fn local_operations_task(args: &OperationsArgs) -> Option<Result<()>> {
    match &args.action {
        OperationsAction::Init { name, directory } => {
            Some(init_task(name, directory.as_deref(), args.json))
        }
        OperationsAction::Check { directory } => {
            Some(check_task(directory.as_deref(), args.json))
        }
        OperationsAction::Test { directory } => Some(test_task(directory.as_deref(), args.json)),
        OperationsAction::Permissions { directory, cloud } => {
            Some(permissions_task(directory.as_deref(), *cloud, args.json))
        }
        OperationsAction::Docs { directory } => Some(docs_task(directory.as_deref(), args.json)),
        OperationsAction::Package { directory } => {
            Some(package_task(directory.as_deref(), args.json))
        }
        #[cfg(feature = "platform")]
        OperationsAction::Publish { .. } | OperationsAction::List | OperationsAction::Invoke { .. } => {
            None
        }
    }
}

#[cfg(feature = "platform")]
pub async fn operations_task(args: OperationsArgs, ctx: ExecutionMode) -> Result<()> {
    if let Some(result) = local_operations_task(&args).await {
        return result;
    }
    platform_action_task(args, ctx).await
}

/// Non-platform builds only ever have local actions to dispatch — `run_cli`
/// intercepts them before context resolution via [`local_operations_task`],
/// so this is unreachable in practice, but the `Commands::Operations` match
/// arm still needs a callable target since that variant isn't itself
/// feature-gated.
#[cfg(not(feature = "platform"))]
pub async fn operations_task(args: OperationsArgs, _ctx: ExecutionMode) -> Result<()> {
    local_operations_task(&args)
        .await
        .expect("every OperationsAction variant is local without the platform feature")
}

#[cfg(feature = "platform")]
async fn platform_action_task(args: OperationsArgs, ctx: ExecutionMode) -> Result<()> {
    let auth = ctx.auth_http().await?;
    let workspace = ctx.resolve_workspace_with_bootstrap(!args.json).await?;
    // The operations catalog is project-scoped: the platform requires a
    // `project` alongside `workspace`. Resolve the linked project (or the
    // `--project` override) the same way the other project-scoped commands do.
    let (_, project_link) = ctx
        .resolve_project(args.project.as_deref(), !args.json)
        .await?;
    let project = project_link.project_id;

    match args.action {
        OperationsAction::Publish { bundle } => {
            publish_task(&auth, &workspace, &project, &bundle, args.json).await
        }
        OperationsAction::List => list_task(&auth, &workspace, &project, args.json).await,
        OperationsAction::Invoke {
            deployment,
            operation,
            params,
            timeout,
            request_access,
            access_duration,
        } => {
            invoke_task(
                &ctx,
                &workspace,
                &project,
                InvokeTaskOptions {
                    deployment: &deployment,
                    operation: &operation,
                    params: &params,
                    timeout_secs: timeout,
                    json: args.json,
                    request_access,
                    access_duration: &access_duration,
                },
            )
            .await
        }
        OperationsAction::Init { .. }
        | OperationsAction::Check { .. }
        | OperationsAction::Test { .. }
        | OperationsAction::Permissions { .. }
        | OperationsAction::Docs { .. }
        | OperationsAction::Package { .. } => {
            unreachable!("local actions are handled by operations_task before this function runs")
        }
    }
}
