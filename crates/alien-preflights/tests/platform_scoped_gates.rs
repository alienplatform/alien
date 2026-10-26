use alien_core::{
    DeploymentConfig, EnvironmentVariablesSnapshot, Platform, ResourceLifecycle, Stack,
    StackInputDefinition, StackSettings, StackState, Storage,
};
use alien_preflights::runner::PreflightRunner;

#[tokio::test]
async fn setup_preparation_only_derives_resources_for_an_available_gate() {
    let mut input = StackInputDefinition::deployer_boolean(
        "managedBucket",
        "Managed bucket",
        "Create a bucket",
        Some(true),
    );
    input.platforms = Some(vec![Platform::Aws]);
    let stack = Stack::new("scoped-gate".into())
        .inputs(vec![input])
        .add_enabled_when(
            Storage::new("data".into()).build(),
            ResourceLifecycle::Frozen,
            "managedBucket",
        )
        .build();
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(Default::default())
        .allow_frozen_changes(false)
        .build();
    let runner = PreflightRunner::new();
    for platform in [
        Platform::Aws,
        Platform::Machines,
        Platform::Kubernetes,
        Platform::Azure,
    ] {
        runner
            .run_template_preflights(&stack, platform)
            .await
            .expect("template checks");
        let prepared = runner
            .apply_mutations(stack.clone(), &StackState::new(platform), &config)
            .await
            .expect("setup preparation");
        if platform == Platform::Aws {
            assert!(prepared.resources.contains_key("data"));
        } else {
            assert!(
                prepared.resources.is_empty(),
                "{platform}: {:?}",
                prepared.resources.keys()
            );
        }
    }
}

#[tokio::test]
async fn render_preparation_removes_only_excluded_container_selections() {
    let mut input = StackInputDefinition::deployer_boolean(
        "enabled",
        "Enabled",
        "Create the container",
        Some(true),
    );
    input.platforms = Some(vec![Platform::Gcp]);
    let stack = Stack::new("scoped-gate".into())
        .inputs(vec![input])
        .add_enabled_when(
            alien_core::Container::new("api".into())
                .code(alien_core::ContainerCode::Image {
                    image: "example.com/api:latest".into(),
                })
                .permissions("execution".into())
                .cpu(alien_core::ResourceSpec {
                    min: "1".into(),
                    desired: "1".into(),
                })
                .memory(alien_core::ResourceSpec {
                    min: "1Gi".into(),
                    desired: "1Gi".into(),
                })
                .build(),
            ResourceLifecycle::Live,
            "enabled",
        )
        .add_enabled_when(
            Storage::new("data".into()).build(),
            ResourceLifecycle::Frozen,
            "enabled",
        )
        .build();
    let mut config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(Default::default())
        .allow_frozen_changes(false)
        .build();
    let selections: alien_core::ComputeSettings = serde_json::from_value(serde_json::json!({
        "containers": { "api": {} }
    }))
    .unwrap();
    config.stack_settings.compute = Some(selections.clone());
    let runner = PreflightRunner::new();
    let state = StackState::new(Platform::Aws);
    let prepared = runner
        .apply_mutations(stack.clone(), &state, &config)
        .await
        .expect("excluded container selection must not prevent rendering");
    assert!(prepared.resources.is_empty());

    for invalid in ["missing", "data"] {
        let mut compute = selections.clone();
        compute
            .containers
            .insert(invalid.into(), Default::default());
        config.stack_settings.compute = Some(compute);
        let error = runner
            .apply_mutations(stack.clone(), &state, &config)
            .await
            .expect_err("unknown and non-container selections must still fail");
        assert!(
            format!("{error:?}").contains("CONTAINER_RESOURCE_SELECTION_INVALID"),
            "{error:?}"
        );
    }
}

#[tokio::test]
async fn build_checks_ignore_excluded_public_container() {
    let mut input = StackInputDefinition::deployer_boolean(
        "enabled",
        "Enabled",
        "Create container",
        Some(true),
    );
    input.platforms = Some(vec![Platform::Aws]);
    let container = alien_core::Container::new("api".into())
        .code(alien_core::ContainerCode::Image {
            image: "example.com/api:latest".into(),
        })
        .permissions("execution".into())
        .cpu(alien_core::ResourceSpec {
            min: "1".into(),
            desired: "1".into(),
        })
        .memory(alien_core::ResourceSpec {
            min: "1Gi".into(),
            desired: "1Gi".into(),
        })
        .public_endpoint(alien_core::PublicEndpoint {
            name: "http".into(),
            port: 8080,
            protocol: alien_core::ExposeProtocol::Http,
            host_label: None,
            wildcard_subdomains: false,
        })
        .build();
    let mut stack = Stack::new("scoped-gate".into())
        .inputs(vec![input])
        .add_enabled_when(container, ResourceLifecycle::Live, "enabled")
        .build();
    stack
        .permissions
        .profiles
        .insert("execution".into(), alien_core::PermissionProfile::new());
    let runner = PreflightRunner::new();
    runner
        .run_build_time_preflights(&stack, Platform::Machines)
        .await
        .expect("excluded public container does not require Machines endpoint support");
    runner
        .run_prebuilt_release_checks(&stack, Platform::Machines)
        .await
        .expect("excluded container also passes prebuilt release checks");
    let mut unscoped = stack;
    unscoped.inputs[0].platforms = None;
    let error = runner
        .run_build_time_preflights(&unscoped, Platform::Machines)
        .await
        .expect_err("an available public container remains unsupported");
    assert!(
        format!("{error:?}").contains("MACHINES_CONTAINER_PUBLIC_ENDPOINT"),
        "{error:?}"
    );
}

#[tokio::test]
async fn ignored_gate_values_are_preflight_warnings_without_values_or_non_gate_inputs() {
    #[derive(Clone)]
    struct LogWriter(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for LogWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let logs = LogWriter(Default::default());
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::WARN)
        .with_writer(move || writer.clone())
        .finish();
    let _subscriber = tracing::subscriber::set_default(subscriber);

    for lifecycle in [ResourceLifecycle::Frozen, ResourceLifecycle::Live] {
        let mut gate = StackInputDefinition::deployer_boolean(
            "enabled",
            "Enabled",
            "Create storage",
            Some(true),
        );
        gate.platforms = Some(vec![Platform::Aws]);
        let mut unused = gate.clone();
        unused.id = "unused".into();
        let stack = Stack::new("scoped-gate".into())
            .inputs(vec![gate, unused])
            .add_enabled_when(Storage::new("data".into()).build(), lifecycle, "enabled")
            .build();
        let mut config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(Default::default())
            .allow_frozen_changes(false)
            .build();
        for supplied in [false, true] {
            config.input_values = if supplied {
                [
                    ("enabled".into(), serde_json::json!("private-value")),
                    ("unused".into(), serde_json::json!(true)),
                ]
                .into()
            } else {
                Default::default()
            };
            logs.0.lock().unwrap().clear();
            let (prepared, summary, _) = PreflightRunner::new()
                .run_deployment_time_preflights(
                    stack.clone(),
                    &StackState::new(Platform::Machines),
                    &config,
                    &alien_core::ClientConfig::Machines,
                    None,
                    None,
                    None,
                )
                .await
                .expect("off-platform storage has no external-binding prerequisite");
            assert!(summary.success);
            assert!(!prepared.resources.contains_key("data"));
            let warnings: Vec<_> = summary
                .results
                .iter()
                .flat_map(|result| &result.warnings)
                .collect();
            assert_eq!(warnings.len(), usize::from(supplied), "{warnings:?}");
            if supplied {
                assert!(warnings[0].contains("'enabled'"));
                assert!(warnings[0].contains("'machines'"));
                assert!(!warnings[0].contains("unused"));
                assert!(!warnings[0].contains("private-value"));
                let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
                assert!(output.contains(warnings[0].as_str()), "{output}");
                assert!(!output.contains("private-value"));
            }
        }
    }
}

#[tokio::test]
async fn legacy_empty_scope_deploys_but_new_releases_are_rejected() {
    let input: StackInputDefinition = serde_json::from_value(serde_json::json!({
        "id": "region", "kind": "string", "providedBy": ["deployer"],
        "required": false, "label": "Region", "description": "Service region",
        "platforms": [], "default": {"type": "string", "value": "us-east-1"}, "env": [{"name": "REGION"}]
    }))
    .unwrap();
    let secret: StackInputDefinition = serde_json::from_value(serde_json::json!({
        "id": "token", "kind": "secret", "providedBy": ["deployer"],
        "required": false, "label": "Token", "description": "Access token", "platforms": []
    }))
    .unwrap();
    let stack = Stack::new("legacy".into())
        .inputs(vec![input, secret])
        .build();
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(Default::default())
        .allow_frozen_changes(false)
        .build();
    let runner = PreflightRunner::new();
    for platform in [Platform::Aws, Platform::Machines] {
        let mut config = config.clone();
        if platform == Platform::Machines {
            config.external_bindings.insert(
                "deployer-secrets",
                alien_core::ExternalBinding::Vault(alien_core::VaultBinding::parameter_store(
                    "legacy",
                )),
            );
        }
        let error = runner
            .run_build_time_preflights(&stack, platform)
            .await
            .unwrap_err();
        assert!(
            format!("{error:?}").contains("platforms must include at least one"),
            "{error:?}"
        );
        let error = runner
            .run_prebuilt_release_checks(&stack, platform)
            .await
            .unwrap_err();
        assert!(
            format!("{error:?}").contains("platforms must include at least one"),
            "{error:?}"
        );
        runner
            .run_template_preflights(&stack, platform)
            .await
            .expect("released template remains valid");
        let (prepared, summary, _) = runner
            .run_deployment_time_preflights(
                stack.clone(),
                &StackState::new(platform),
                &config,
                &alien_core::ClientConfig::Machines,
                None,
                None,
                None,
            )
            .await
            .expect("existing release remains deployable");
        assert!(summary.success);
        let variables = alien_core::resolve_stack_input_environment_variables(
            &prepared.inputs,
            &config.input_values,
            platform,
        )
        .unwrap();
        assert_eq!(variables.len(), 1);
        assert_eq!(variables[0].name, "REGION");
        assert_eq!(variables[0].value, "us-east-1");
        let slots = alien_core::deployer_secret_slots(
            &prepared.inputs,
            &config.input_values,
            Some(&[]),
            &[],
            platform,
        );
        assert_eq!(slots.len(), 1);
    }
}
