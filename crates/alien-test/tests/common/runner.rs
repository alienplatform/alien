//! `check_all_bindings()` — orchestrator that iterates the support matrix
//! and calls the appropriate check function for each supported binding.

use alien_core::Platform;
use alien_test::{Binding, DeploymentModel, TestDeployment};
use tracing::{info, warn};

use super::bindings;
use alien_test::e2e;

/// Run all binding checks that are supported for the given platform and model.
///
/// For each binding in `supported_bindings(platform, model)`:
/// - If the binding has a known exclusion, log it and skip.
/// - Otherwise, call the matching check function.
pub async fn check_all_bindings(
    deployment: &TestDeployment,
    platform: Platform,
    model: DeploymentModel,
) -> anyhow::Result<()> {
    let supported = e2e::supported_bindings(platform, model);
    info!(
        platform = %platform.as_str(),
        model = %model,
        binding_count = supported.len(),
        "Running binding checks"
    );

    for binding in &supported {
        // Check for known exclusions
        if let Some(reason) = e2e::exclusion_reason(platform, model, *binding) {
            warn!(
                binding = %binding,
                reason = %reason,
                "Skipping excluded binding"
            );
            continue;
        }

        info!(binding = %binding, "Checking binding");

        match binding {
            Binding::Health => bindings::check_health(deployment).await?,
            Binding::Hello => bindings::check_hello(deployment).await?,
            Binding::Sse => bindings::check_sse(deployment).await?,
            Binding::Environment => bindings::check_environment(deployment).await?,
            Binding::Inspect => bindings::check_inspect(deployment).await?,
            Binding::ExternalSecret => bindings::check_external_secret(deployment).await?,
            Binding::Events => bindings::check_events(deployment).await?,
            Binding::Storage => bindings::check_storage(deployment).await?,
            Binding::Kv => bindings::check_kv(deployment).await?,
            Binding::Vault => bindings::check_vault(deployment).await?,
            Binding::Queue => bindings::check_queue(deployment).await?,
            Binding::Function => bindings::check_function(deployment).await?,
            Binding::Container => bindings::check_container(deployment).await?,
            Binding::WaitUntil => bindings::check_wait_until(deployment).await?,
        }

        info!(binding = %binding, "Binding check passed");
    }

    info!(
        platform = %platform.as_str(),
        model = %model,
        "All binding checks passed"
    );
    Ok(())
}
