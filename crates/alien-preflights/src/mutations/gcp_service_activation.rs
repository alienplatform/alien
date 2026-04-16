//! GCP Service Activation mutation that enables required GCP APIs.

use crate::error::Result;
use crate::StackMutation;
use alien_core::{
    DeploymentConfig, Platform, ResourceEntry, ResourceLifecycle, ServiceActivation, Stack,
    StackState,
};
use async_trait::async_trait;
use std::collections::HashMap;
use tracing::{debug, info};

/// Mutation that adds ServiceActivation resources for required GCP APIs.
///
/// Different GCP resource types require different APIs to be enabled:
/// - function: run.googleapis.com (Cloud Run)
/// - build: cloudbuild.googleapis.com (Cloud Build)
/// - storage: storage.googleapis.com (Cloud Storage)
/// - role: iam.googleapis.com + cloudresourcemanager.googleapis.com
/// - artifact-registry: artifactregistry.googleapis.com
/// - kv: firestore.googleapis.com (Firestore)
/// - queue: pubsub.googleapis.com (Pub/Sub)
/// - vault: secretmanager.googleapis.com (Secret Manager)
/// - network: compute.googleapis.com (Compute Engine)
/// - container-cluster: compute.googleapis.com (Compute Engine)
pub struct GcpServiceActivationMutation;

#[async_trait]
impl StackMutation for GcpServiceActivationMutation {
    fn description(&self) -> &'static str {
        "Enable required GCP APIs for resources"
    }

    fn should_run(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> bool {
        // Only add for GCP platform
        if stack_state.platform != Platform::Gcp {
            return false;
        }

        // Check what resource types exist in the stack that need service activation
        let required_services = self.get_required_services(stack);

        if required_services.is_empty() {
            return false;
        }

        // Check if all required service activations already exist
        let existing_services: std::collections::HashSet<_> = stack
            .resources
            .iter()
            .filter_map(|(_id, entry)| {
                if let Some(service) = entry.config.downcast_ref::<ServiceActivation>() {
                    Some(service.service_name.clone())
                } else {
                    None
                }
            })
            .collect();

        // Return true if any required service is missing
        required_services
            .values()
            .any(|service_name| !existing_services.contains(service_name))
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        _stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        info!("Adding GCP ServiceActivation resources");

        let required_services = self.get_required_services(&stack);

        for (service_id, service_name) in required_services {
            // Check if this service activation already exists
            if stack
                .resources
                .iter()
                .any(|(existing_id, _)| existing_id == &service_id)
            {
                continue;
            }

            // Create the ServiceActivation resource
            let service_activation = ServiceActivation::new(service_id.clone())
                .service_name(service_name.clone())
                .build();

            // Add it to the stack as a frozen resource
            let service_entry = ResourceEntry {
                config: alien_core::Resource::new(service_activation),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(), // GCP service activations don't depend on other resources
                remote_access: false,
            };

            stack.resources.insert(service_id.clone(), service_entry);
            debug!(
                "Added ServiceActivation resource '{}' for API '{}'",
                service_id, service_name
            );
        }

        Ok(stack)
    }
}

impl GcpServiceActivationMutation {
    /// Get the mapping of service activation ID to API name based on resources in the stack
    fn get_required_services(&self, stack: &Stack) -> HashMap<String, String> {
        let mut services = HashMap::new();

        for (_, entry) in &stack.resources {
            let resource_type = entry.config.resource_type();
            match resource_type.as_ref() {
                "function" => {
                    services.insert(
                        "enable-cloud-run".to_string(),
                        "run.googleapis.com".to_string(),
                    );
                }
                "build" => {
                    services.insert(
                        "enable-cloud-build".to_string(),
                        "cloudbuild.googleapis.com".to_string(),
                    );
                }
                "storage" => {
                    services.insert(
                        "enable-cloud-storage".to_string(),
                        "storage.googleapis.com".to_string(),
                    );
                }
                "role" => {
                    services.insert("enable-iam".to_string(), "iam.googleapis.com".to_string());
                    services.insert(
                        "enable-cloud-resource-manager".to_string(),
                        "cloudresourcemanager.googleapis.com".to_string(),
                    );
                }
                "artifact-registry" => {
                    services.insert(
                        "enable-artifact-registry".to_string(),
                        "artifactregistry.googleapis.com".to_string(),
                    );
                }
                "kv" => {
                    services.insert(
                        "enable-firestore".to_string(),
                        "firestore.googleapis.com".to_string(),
                    );
                }
                "queue" => {
                    services.insert(
                        "enable-pubsub".to_string(),
                        "pubsub.googleapis.com".to_string(),
                    );
                }
                "vault" => {
                    services.insert(
                        "enable-secret-manager".to_string(),
                        "secretmanager.googleapis.com".to_string(),
                    );
                }
                "network" => {
                    services.insert(
                        "enable-compute-engine".to_string(),
                        "compute.googleapis.com".to_string(),
                    );
                }
                "container-cluster" => {
                    services.insert(
                        "enable-compute-engine".to_string(),
                        "compute.googleapis.com".to_string(),
                    );
                    // Required for the IMDS metadata proxy to impersonate service accounts on
                    // behalf of containers via generateAccessToken.
                    services.insert(
                        "enable-iam-credentials".to_string(),
                        "iamcredentials.googleapis.com".to_string(),
                    );
                }
                _ => {}
            }
        }

        services
    }
}
