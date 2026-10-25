use alien_core::{
    AwsManagementConfig, AzureManagementConfig, ComputeBackend, Container, ContainerCode,
    DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, GcpFederationTrust,
    GcpManagementConfig, HorizonConfig, ManagementConfig, PermissionProfile, PermissionsConfig,
    Platform, ResourceEntry, ResourceLifecycle, ResourceSpec, Stack, StackSettings, StackState,
    Storage, Vault, Worker, WorkerCode,
};
use alien_preflights::runner::PreflightRunner;

#[tokio::test]
async fn generated_resources_keep_names_across_preparation() {
    for platform in [
        Platform::Aws,
        Platform::Gcp,
        Platform::Azure,
        Platform::Kubernetes,
        Platform::Machines,
    ] {
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
            .add_entry(
                ResourceEntry::new(container, ResourceLifecycle::Live).display_name("API server"),
            )
            .permissions(
                PermissionsConfig::new().with_profile("application", PermissionProfile::new()),
            )
            .build();
        if matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure) {
            let storage = Storage::new("uploads".into()).build();
            stack.resources.insert(
                "uploads".into(),
                ResourceEntry::new(storage, ResourceLifecycle::Frozen)
                    .display_name("Uploads bucket"),
            );
            let vault = Vault::new("credentials".into()).build();
            stack.resources.insert(
                "credentials".into(),
                ResourceEntry::new(vault, ResourceLifecycle::Frozen)
                    .display_name("Service credentials"),
            );
            let worker = Worker::new("events".into())
                .code(WorkerCode::Image {
                    image: "ghcr.io/acme/events:1".into(),
                })
                .permissions("application".into())
                .build();
            stack.resources.insert(
                "events".into(),
                ResourceEntry::new(worker, ResourceLifecycle::Live).display_name("Event processor"),
            );
        }
        let management = match platform {
            Platform::Aws => Some(ManagementConfig::Aws(AwsManagementConfig {
                managing_role_arn: "arn:aws:iam::111122223333:role/management".into(),
            })),
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
        let config = DeploymentConfig::builder()
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
        let runner = PreflightRunner::new();
        let checks = runner
            .run_compile_time_checks(&stack, platform)
            .await
            .expect("compile checks run");
        assert!(checks.success, "{platform:?}: {:?}", checks.get_errors());
        let state = StackState::new(platform);
        let prepared = runner
            .apply_mutations(stack.clone(), &state, &config)
            .await
            .expect("real mutations prepare stack");
        assert!(prepared.resources.len() > stack.resources.len());
        for (id, entry) in &prepared.resources {
            assert!(
                entry
                    .display_name
                    .as_ref()
                    .is_some_and(|name| !name.is_empty()),
                "{platform:?}/{id}"
            );
            if !stack.resources.contains_key(id) {
                assert!(
                    entry
                        .description
                        .as_ref()
                        .is_some_and(|text| !text.is_empty()),
                    "{platform:?}/{id}"
                );
            }
        }
        let repeated = runner
            .apply_mutations(prepared.clone(), &state, &config)
            .await
            .expect("preparation can repeat");
        for (id, entry) in &prepared.resources {
            assert_eq!(entry.display_name, repeated.resources[id].display_name);
        }
        if let Ok(directory) = std::env::var("PREFLIGHT_EVIDENCE_DIR") {
            std::fs::write(
                std::path::Path::new(&directory).join(format!("{platform}-preflight.json")),
                serde_json::to_vec_pretty(&prepared).unwrap(),
            )
            .unwrap();
            std::fs::write(
                std::path::Path::new(&directory).join(format!("{platform}-declared.json")),
                serde_json::to_vec_pretty(&stack).unwrap(),
            )
            .unwrap();
        }
    }
}
