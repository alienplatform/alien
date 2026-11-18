//! The service activations Alien's own resource types add, per cloud.
//!
//! Each cloud's activations are declared once below. The declaration gives each one a name for
//! the per-type rules and lists it in the cloud's table, and nothing else can build an
//! [`Activation`], so every activation a per-type rule adds is in the table. An extension type
//! that asks for one of these services gets the same activation and id.

/// One service activation Alien's own types add: its id, and the service it enables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Activation {
    id: &'static str,
    service: &'static str,
}

impl Activation {
    pub(crate) const fn id(&self) -> &'static str {
        self.id
    }

    pub(crate) const fn service(&self) -> &'static str {
        self.service
    }
}

macro_rules! activations {
    ($($name:ident = ($id:literal, $service:literal),)*) => {
        $(pub(crate) const $name: Activation = Activation { id: $id, service: $service };)*

        /// Every activation declared for this cloud.
        pub(crate) const ALL: &[Activation] = &[$($name),*];
    };
}

/// GCP APIs.
pub(crate) mod gcp {
    use super::Activation;

    activations! {
        CLOUD_QUOTAS = ("enable-cloud-quotas", "cloudquotas.googleapis.com"),
        CLOUD_RUN = ("enable-cloud-run", "run.googleapis.com"),
        CLOUD_BUILD = ("enable-cloud-build", "cloudbuild.googleapis.com"),
        CLOUD_STORAGE = ("enable-cloud-storage", "storage.googleapis.com"),
        CLOUD_KMS = ("enable-cloud-kms", "cloudkms.googleapis.com"),
        IAM = ("enable-iam", "iam.googleapis.com"),
        CLOUD_RESOURCE_MANAGER = (
            "enable-cloud-resource-manager",
            "cloudresourcemanager.googleapis.com"
        ),
        IAM_CREDENTIALS = ("enable-iam-credentials", "iamcredentials.googleapis.com"),
        SECURITY_TOKEN_SERVICE = ("enable-security-token-service", "sts.googleapis.com"),
        ARTIFACT_REGISTRY = ("enable-artifact-registry", "artifactregistry.googleapis.com"),
        FIRESTORE = ("enable-firestore", "firestore.googleapis.com"),
        AI_PLATFORM = ("enable-aiplatform", "aiplatform.googleapis.com"),
        PUBSUB = ("enable-pubsub", "pubsub.googleapis.com"),
        SECRET_MANAGER = ("enable-secret-manager", "secretmanager.googleapis.com"),
        CLOUD_SQL = ("enable-cloud-sql", "sqladmin.googleapis.com"),
        COMPUTE_ENGINE = ("enable-compute-engine", "compute.googleapis.com"),
        CONTAINER = ("enable-container", "container.googleapis.com"),
        MEMORYSTORE = ("enable-memorystore", "memorystore.googleapis.com"),
        NETWORK_CONNECTIVITY = (
            "enable-network-connectivity",
            "networkconnectivity.googleapis.com"
        ),
        SERVICE_CONSUMER_MANAGEMENT = (
            "enable-service-consumer-management",
            "serviceconsumermanagement.googleapis.com"
        ),
    }
}

/// Azure resource providers.
pub(crate) mod azure {
    use super::Activation;

    activations! {
        APP = ("enable-app", "Microsoft.App"),
        EVENT_GRID = ("enable-eventgrid", "Microsoft.EventGrid"),
        SERVICE_BUS = ("enable-servicebus", "Microsoft.ServiceBus"),
        STORAGE = ("enable-storage", "Microsoft.Storage"),
        KEY_VAULT = ("enable-keyvault", "Microsoft.KeyVault"),
        CONTAINER_REGISTRY = ("enable-container-registry", "Microsoft.ContainerRegistry"),
        POSTGRESQL = ("enable-postgresql", "Microsoft.DBforPostgreSQL"),
        CONTAINER_SERVICE = ("enable-container-service", "Microsoft.ContainerService"),
        NETWORK = ("enable-network", "Microsoft.Network"),
        CACHE = ("enable-cache", "Microsoft.Cache"),
    }
}

/// Alien's own per-type rules, on stacks that reach each of them, against an extension type that
/// asks for the same services.
#[cfg(test)]
mod tests {
    use super::{azure, gcp};
    use crate::runner::PreflightRunner;
    use alien_core::{
        ArtifactRegistry, AzureManagementConfig, ComputeBackend, Container, ContainerCode,
        CustomResource, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings,
        GcpFederationTrust, GcpManagementConfig, HorizonConfig, Key, Kv, ManagementConfig,
        PermissionProfile, PermissionsConfig, Platform, Postgres, Queue, ResourceEntry,
        ResourceLifecycle, ResourceSpec, ServiceActivation, SetupManagement, Stack, StackSettings,
        StackState, Storage, Vault, Worker, WorkerCode,
    };
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::collections::BTreeSet;

    fn kitchen_sink() -> Stack {
        let container = Container::new("api".into())
            .code(ContainerCode::Image {
                image: "ghcr.io/acme/api:1".into(),
            })
            .cpu(ResourceSpec {
                min: "0.25".into(),
                desired: "0.5".into(),
            })
            .memory(ResourceSpec {
                min: "512Mi".into(),
                desired: "1Gi".into(),
            })
            .permissions("application".into())
            .port(8080)
            .build();
        let mut stack = Stack::new("acme".into())
            .add_entry(ResourceEntry::new(container, ResourceLifecycle::Live))
            .permissions(
                PermissionsConfig::new().with_profile("application", PermissionProfile::new()),
            )
            .build();
        let mut add = |id: &str, entry: ResourceEntry| {
            stack.resources.insert(id.into(), entry);
        };
        add(
            "uploads",
            ResourceEntry::new(
                Storage::new("uploads".into()).build(),
                ResourceLifecycle::Frozen,
            ),
        );
        add(
            "credentials",
            ResourceEntry::new(
                Vault::new("credentials".into()).build(),
                ResourceLifecycle::Frozen,
            ),
        );
        add(
            "events",
            ResourceEntry::new(
                Worker::new("events".into())
                    .code(WorkerCode::Image {
                        image: "ghcr.io/acme/events:1".into(),
                    })
                    .permissions("application".into())
                    .build(),
                ResourceLifecycle::Live,
            ),
        );
        add(
            "cache",
            ResourceEntry::new(Kv::new("cache".into()).build(), ResourceLifecycle::Frozen),
        );
        add(
            "jobs",
            ResourceEntry::new(Queue::new("jobs".into()).build(), ResourceLifecycle::Frozen),
        );
        add(
            "db",
            ResourceEntry::new(
                Postgres::new("db".into()).build(),
                ResourceLifecycle::Frozen,
            ),
        );
        add(
            "redis",
            ResourceEntry::new(
                alien_core::Redis::new("redis".into()).build(),
                ResourceLifecycle::Frozen,
            ),
        );
        add(
            "signing",
            ResourceEntry::new(
                Key::new("signing".into()).build(),
                ResourceLifecycle::Frozen,
            ),
        );
        add(
            "images",
            ResourceEntry::new(
                ArtifactRegistry::new("images".into()).build(),
                ResourceLifecycle::Frozen,
            ),
        );
        stack
    }

    /// Stacks for the per-type rules the kitchen sink does not reach.
    fn extras(platform: Platform) -> Vec<Stack> {
        use alien_core::{
            Ai, Build, KubernetesCluster, KubernetesClusterOwnership, KubernetesClusterProvider,
            KubernetesHeartbeatMode, WorkerTrigger,
        };
        let storage = Storage::new("uploads".into()).build();
        let worker = Worker::new("processor".into())
            .code(WorkerCode::Image {
                image: "test:latest".into(),
            })
            .permissions("application".into())
            .trigger(WorkerTrigger::storage(&storage, vec!["created".into()]))
            .build();
        vec![
            Stack::new("ai".into())
                .add(Ai::new("models".into()).build(), ResourceLifecycle::Frozen)
                .build(),
            Stack::new("build".into())
                .add(
                    Build::new("builder".into())
                        .permissions("application".into())
                        .build(),
                    ResourceLifecycle::Frozen,
                )
                .permissions(
                    PermissionsConfig::new().with_profile("application", PermissionProfile::new()),
                )
                .build(),
            Stack::new("trigger".into())
                .add(storage, ResourceLifecycle::Frozen)
                .add(worker, ResourceLifecycle::Live)
                .permissions(
                    PermissionsConfig::new().with_profile("application", PermissionProfile::new()),
                )
                .build(),
            Stack::new("cluster".into())
                .add(
                    KubernetesCluster::new("kubernetes".into())
                        .provider(if platform == Platform::Gcp {
                            KubernetesClusterProvider::Gke
                        } else {
                            KubernetesClusterProvider::Aks
                        })
                        .ownership(KubernetesClusterOwnership::Managed)
                        .namespace("default".into())
                        .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
                        .build(),
                    ResourceLifecycle::Frozen,
                )
                .build(),
        ]
    }

    fn small() -> Stack {
        Stack::new("small".into())
            .add(
                Storage::new("files".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .build()
    }

    fn config(
        platform: Platform,
        base: Option<Platform>,
        setup: SetupManagement,
    ) -> DeploymentConfig {
        let management = match base.unwrap_or(platform) {
            Platform::Gcp => Some(ManagementConfig::Gcp(GcpManagementConfig {
                service_account_email: "manager@acme.iam.gserviceaccount.com".into(),
                federation_trust: Some(GcpFederationTrust {
                    issuer: "https://accounts.google.com".into(),
                    subject: "123456789012345678901".into(),
                }),
            })),
            Platform::Azure => Some(ManagementConfig::Azure(AzureManagementConfig {
                managing_tenant_id: "11111111-1111-1111-1111-111111111111".into(),
                oidc_issuer: "https://issuer.example".into(),
                oidc_subject: "manager".into(),
            })),
            _ => None,
        };
        let mut config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .maybe_management_config(management)
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: "2026-10-25T00:00:00Z".into(),
            })
            .compute_backend(ComputeBackend::Horizon(HorizonConfig {
                url: "https://horizon.example".into(),
                horizon_machine_image: None,
                clusters: Default::default(),
            }))
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        config.stack_settings.setup_management = setup;
        config.base_platform = base;
        config
    }

    async fn prepare(
        stack: Stack,
        platform: Platform,
        base: Option<Platform>,
    ) -> Result<Stack, String> {
        PreflightRunner::new()
            .apply_mutations(
                stack,
                &StackState::new(platform),
                &config(platform, base, SetupManagement::Manual),
            )
            .await
            .map_err(|e| e.to_string())
    }

    fn activations(stack: &Stack) -> BTreeMap<String, Vec<String>> {
        let mut out = BTreeMap::<String, Vec<String>>::new();
        for (id, entry) in stack.resources() {
            if let Some(a) = entry.config.downcast_ref::<ServiceActivation>() {
                out.entry(a.service_name.to_ascii_lowercase())
                    .or_default()
                    .push(id.clone());
            }
        }
        out
    }

    fn with_extension(mut stack: Stack, types: &[(&str, serde_json::Value)]) -> Stack {
        let mut resource_types = serde_json::Map::new();
        for (i, (name, services)) in types.iter().enumerate() {
            let resource_type = format!("acme/{name}");
            resource_types.insert(
                resource_type.clone(),
                json!({
                    "lifecycles": ["frozen", "live"],
                    "holdsData": false,
                    "setup": { "cloudformation": ["aws"], "terraform": ["aws", "gcp", "azure"], "helm": ["kubernetes"], "importers": ["aws", "gcp", "azure"] },
                    "controllerPlatforms": ["aws", "gcp", "azure"],
                    "permissionSets": [],
                    "link": { "permissionSets": [] },
                    "schemas": { "config": { "type": "object" }, "outputs": { "type": "object" } },
                    "services": services,
                }),
            );
            let id = format!("custom-{i}");
            let resource =
                CustomResource::from_untagged(&resource_type, json!({ "id": id })).unwrap();
            stack
                .resources
                .insert(id, ResourceEntry::new(resource, ResourceLifecycle::Frozen));
        }
        stack.extensions = vec![serde_json::from_value(json!({
            "name": "acme", "version": "0.1.0", "digest": "sha256:4ff9", "resourceTypes": resource_types
        }))
        .unwrap()];
        stack
    }

    /// Every service a built-in stack here enables, asked for by an extension type: one
    /// activation each, under Alien's id, with or without the Alien types that need it. The
    /// stacks reach every declared activation.
    #[tokio::test]
    async fn every_service_alien_s_own_rules_enable_keeps_alien_s_activation() {
        for (platform, declared) in [(Platform::Gcp, gcp::ALL), (Platform::Azure, azure::ALL)] {
            let key = if platform == Platform::Gcp {
                "gcp"
            } else {
                "azure"
            };
            let mut reached = BTreeSet::new();
            for stack in std::iter::once(kitchen_sink()).chain(extras(platform)) {
                let alien = activations(&prepare(stack.clone(), platform, None).await.unwrap());
                reached.extend(alien.values().flatten().cloned());
                let names: Vec<String> = alien.keys().cloned().collect();
                // The services alone, without the Alien types that need them.
                let only = with_extension(small(), &[("a", json!({ key: names.clone() }))]);
                let got = activations(&prepare(only, platform, None).await.unwrap());
                for (service, ids) in &alien {
                    assert_eq!(&got[service], ids, "{platform:?} {}: {service}", stack.id());
                }
                // With the Alien types: no second activation.
                let both = with_extension(stack.clone(), &[("a", json!({ key: names }))]);
                let got = activations(&prepare(both, platform, None).await.unwrap());
                assert_eq!(got, alien, "{platform:?} {}", stack.id());
            }
            // So a declared activation no stack here reaches fails this test, not only a future
            // stack's.
            let declared: BTreeSet<String> = declared
                .iter()
                .map(|activation| activation.id().to_string())
                .collect();
            assert_eq!(reached, declared, "{platform:?}");
        }
    }

    /// A type that spells an Azure provider in lower case still waits for Alien's activation of it.
    #[tokio::test]
    async fn a_lower_case_azure_provider_still_waits_for_the_activation() {
        let stack = with_extension(small(), &[("a", json!({ "azure": ["microsoft.network"] }))]);
        let prepared = prepare(stack, Platform::Azure, None).await.unwrap();
        let deps: Vec<String> = prepared.resources["custom-0"]
            .dependencies
            .iter()
            .map(|r| r.id.clone())
            .collect();
        assert!(deps.contains(&"enable-network".to_string()), "{deps:?}");
    }
}
