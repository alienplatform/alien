//! GCP Service Activation mutation that enables required GCP APIs.

use crate::error::Result;
use crate::mutations::runs_on_platform_or_base;
use crate::StackMutation;
use alien_core::{
    DeploymentConfig, Key, Platform, ResourceEntry, ResourceLifecycle, ServiceActivation, Stack,
    StackState,
};
use async_trait::async_trait;
use std::collections::BTreeMap;
use tracing::{debug, info};

/// Mutation that adds ServiceActivation resources for required GCP APIs.
///
/// Different GCP resource types require different APIs to be enabled:
/// - worker: run.googleapis.com (Cloud Run)
/// - build: cloudbuild.googleapis.com (Cloud Build)
/// - storage: storage.googleapis.com (Cloud Storage)
/// - role: iam.googleapis.com + cloudresourcemanager.googleapis.com
/// - artifact-registry: artifactregistry.googleapis.com
/// - kv: firestore.googleapis.com (Firestore)
/// - ai: aiplatform.googleapis.com (Vertex AI)
/// - queue: pubsub.googleapis.com (Pub/Sub)
/// - vault: secretmanager.googleapis.com (Secret Manager)
/// - postgres: sqladmin.googleapis.com (Cloud SQL) + compute.googleapis.com (PSC) + secretmanager.googleapis.com (connection secret)
/// - network: compute.googleapis.com (Compute Engine)
/// - compute-cluster: compute.googleapis.com (Compute Engine)
// Google documents these prerequisites for customer-owned workload federation.
// Setup enables them; runtime identities do not receive Service Usage administration.
pub(crate) const GCP_FEDERATION_SERVICES: [(&str, &str); 4] = [
    ("enable-iam", "iam.googleapis.com"),
    (
        "enable-cloud-resource-manager",
        "cloudresourcemanager.googleapis.com",
    ),
    ("enable-iam-credentials", "iamcredentials.googleapis.com"),
    ("enable-security-token-service", "sts.googleapis.com"),
];

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
        config: &DeploymentConfig,
    ) -> bool {
        if !runs_on_platform_or_base(stack_state, config, Platform::Gcp) {
            return false;
        }

        // Check what resource types exist in the stack that need service activation
        let required_services = self.get_required_services(stack, stack_state);

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
        stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        info!("Adding GCP ServiceActivation resources");

        let required_services = self.get_required_services(&stack, stack_state);

        for (service_id, service_name) in required_services {
            // Check if this service activation already exists
            if stack
                .resources
                .iter()
                .any(|(existing_id, _)| existing_id == &service_id)
            {
                continue;
            }
            if service_id == CLOUD_KMS_ACTIVATION_ID && kms_used_without_activation(stack_state) {
                debug!(
                    "Cloud KMS is already in use by the installed Key; not adding its activation"
                );
                continue;
            }

            // Create the ServiceActivation resource
            let service_activation = ServiceActivation::new(service_id.clone())
                .service_name(service_name.clone())
                .build();

            // Add it to the stack as a frozen resource
            let service_entry = ResourceEntry {
                enabled_when: None,
                config: alien_core::Resource::new(service_activation),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(), // GCP service activations don't depend on other resources
                display_name: Some("Required Google Cloud API".to_string()),
                description: Some(
                    "Enables a Google Cloud API required by this deployment.".to_string(),
                ),
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

/// The Cloud KMS activation. Its id is also the Key's dependency.
pub(crate) const CLOUD_KMS_ACTIVATION_ID: &str = "enable-cloud-kms";

/// Whether the installed stack already has a Key but no Cloud KMS activation.
///
/// Setups from before the activation existed created the key ring through the Cloud KMS API, so
/// the API is already enabled in that project. Adding the activation now would be a new
/// setup-owned resource, which would make the next update require setup for nothing.
///
/// A Terraform setup rerun prepares its import from empty state, so it adds the activation, and
/// later updates keep it. A direct setup update prepares from the installed state and keeps
/// leaving it out; GCP Keys are imported from Terraform, and the API is already enabled anyway.
fn kms_used_without_activation(stack_state: &StackState) -> bool {
    stack_state
        .resources
        .values()
        .any(|resource| resource.resource_type == Key::RESOURCE_TYPE.as_ref())
        && !stack_state.resources.contains_key(CLOUD_KMS_ACTIVATION_ID)
}

impl GcpServiceActivationMutation {
    /// Get the mapping of service activation ID to API name based on resources in the stack.
    /// Ordered, because `mutate` inserts in this order and setup renders the stack in it.
    fn get_required_services(
        &self,
        stack: &Stack,
        stack_state: &StackState,
    ) -> BTreeMap<String, String> {
        let mut services = BTreeMap::new();
        // Enable quota reads in new setup artifacts. An installed deployment
        // without this optional API must keep updating with Unknown warnings;
        // adding a Frozen resource would require setup permissions it may lack.
        if stack_state.resources.is_empty()
            || stack_state.resources.contains_key("enable-cloud-quotas")
        {
            services.insert(
                "enable-cloud-quotas".to_string(),
                "cloudquotas.googleapis.com".to_string(),
            );
        }

        for (_, entry) in &stack.resources {
            let resource_type = entry.config.resource_type();
            match resource_type.as_ref() {
                "remote-stack-management" | "resource-access" => {
                    for (id, service) in GCP_FEDERATION_SERVICES {
                        services.insert(id.to_string(), service.to_string());
                    }
                }

                "worker" => {
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
                "key" => {
                    services.insert(
                        CLOUD_KMS_ACTIVATION_ID.to_string(),
                        "cloudkms.googleapis.com".to_string(),
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
                "ai" => {
                    // Vertex AI. The native controller enables this at runtime, but a
                    // Frozen/Terraform GCP deploy relies on this activation being injected
                    // here, otherwise the first Vertex call fails with SERVICE_DISABLED.
                    services.insert(
                        "enable-aiplatform".to_string(),
                        "aiplatform.googleapis.com".to_string(),
                    );
                }
                "sandbox" => {
                    // Agent Platform sandboxes are Vertex reasoning engines; a stack with a
                    // sandbox but no ai resource would otherwise never enable aiplatform. Same
                    // key as "ai", so a stack with both enables it once.
                    services.insert(
                        "enable-aiplatform".to_string(),
                        "aiplatform.googleapis.com".to_string(),
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
                "postgres" => {
                    services.insert(
                        "enable-cloud-sql".to_string(),
                        "sqladmin.googleapis.com".to_string(),
                    );
                    // The private connection uses Private Service Connect — a
                    // `google_compute_address` + `google_compute_forwarding_rule` in the stack
                    // VPC — so Compute Engine must be enabled too. Shares the `network` arm's key
                    // so the two dedupe when both resources are present.
                    services.insert(
                        "enable-compute-engine".to_string(),
                        "compute.googleapis.com".to_string(),
                    );
                    // The setup emitter stores the generated connection password in a
                    // `google_secret_manager_secret`, so the apply needs the API even when the
                    // stack has no `vault`. Shares the `vault` arm's key so the two dedupe.
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
                "compute-cluster" => {
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
                "kubernetes-cluster" => {
                    services.insert(
                        "enable-container".to_string(),
                        "container.googleapis.com".to_string(),
                    );
                    services.insert(
                        "enable-compute-engine".to_string(),
                        "compute.googleapis.com".to_string(),
                    );
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

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{Ai, ResourceLifecycle, Stack};

    #[tokio::test]
    async fn quota_activation_is_added_once_for_native_and_imported_gcp_setup() {
        let mutation = GcpServiceActivationMutation;
        let config = DeploymentConfig::builder()
            .stack_settings(alien_core::StackSettings::default())
            .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: String::new(),
                created_at: "2024-01-01T00:00:00Z".into(),
            })
            .allow_frozen_changes(false)
            .external_bindings(alien_core::ExternalBindings::default())
            .build();
        for platform in [Platform::Gcp, Platform::Kubernetes] {
            let mut config = config.clone();
            if platform == Platform::Kubernetes {
                config.base_platform = Some(Platform::Gcp);
            }
            let state = StackState::new(platform);
            let stack = Stack::new("quota-activation".into()).build();
            assert!(mutation.should_run(&stack, &state, &config));
            let stack = mutation.mutate(stack, &state, &config).await.unwrap();
            assert_eq!(stack.resources.len(), 1);
            let entry = &stack.resources["enable-cloud-quotas"];
            let activation = entry.config.downcast_ref::<ServiceActivation>().unwrap();
            assert_eq!(activation.service_name, "cloudquotas.googleapis.com");
            assert_eq!(entry.lifecycle, ResourceLifecycle::Frozen);
            assert!(!mutation.should_run(&stack, &state, &config));
            let repeated = mutation
                .mutate(stack.clone(), &state, &config)
                .await
                .unwrap();
            assert_eq!(
                serde_json::to_value(repeated).unwrap(),
                serde_json::to_value(stack).unwrap()
            );
        }
        for platform in [
            Platform::Aws,
            Platform::Azure,
            Platform::Local,
            Platform::Kubernetes,
        ] {
            let state = StackState::new(platform);
            let stack = Stack::new("no-gcp".into()).build();
            assert!(!mutation.should_run(&stack, &state, &config));
        }
    }

    #[tokio::test]
    async fn installed_stack_without_quota_activation_keeps_optional_api_absent() {
        let config = DeploymentConfig::builder()
            .stack_settings(alien_core::StackSettings::default())
            .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: String::new(),
                created_at: "2024-01-01T00:00:00Z".into(),
            })
            .allow_frozen_changes(false)
            .external_bindings(alien_core::ExternalBindings::default())
            .build();
        let stack = Stack::new("installed".into())
            .add(Ai::new("llm".into()).build(), ResourceLifecycle::Frozen)
            .build();
        let mut state = StackState::new(Platform::Gcp);
        let entry = &stack.resources["llm"];
        state.resources.insert(
            "llm".into(),
            alien_core::StackResourceState::new_pending(
                entry.config.resource_type().to_string(),
                entry.config.clone(),
                Some(entry.lifecycle),
                Vec::new(),
            ),
        );
        // Roundtrip installed state without the optional activation.
        let state: StackState =
            serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
        let mutation = GcpServiceActivationMutation;
        assert!(mutation.should_run(&stack, &state, &config));
        let prepared = mutation.mutate(stack, &state, &config).await.unwrap();
        assert!(!prepared.resources.contains_key("enable-cloud-quotas"));
        assert!(prepared.resources.contains_key("enable-aiplatform"));
        assert!(!mutation.should_run(&prepared, &state, &config));
        let repeated = mutation
            .mutate(prepared.clone(), &state, &config)
            .await
            .unwrap();
        assert_eq!(repeated, prepared);
    }

    #[test]
    fn ai_resource_requires_vertex_aiplatform() {
        let stack = Stack::new("test-stack".to_string())
            .add(
                Ai::new("llm".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let services = GcpServiceActivationMutation
            .get_required_services(&stack, &StackState::new(Platform::Gcp));
        assert_eq!(
            services.get("enable-aiplatform").map(String::as_str),
            Some("aiplatform.googleapis.com"),
            "a GCP AI resource must inject Vertex AI enablement for the Terraform/Frozen path"
        );
    }
}
