use super::*;
use clap::Parser;
use std::io::Write;

#[test]
fn cloud_push_platforms_require_install_context() {
    assert!(requires_install_context(Platform::Aws));
    assert!(requires_install_context(Platform::Gcp));
    assert!(requires_install_context(Platform::Azure));
}

#[test]
fn pull_model_platforms_do_not_require_install_context() {
    assert!(!requires_install_context(Platform::Kubernetes));
    assert!(!requires_install_context(Platform::Machines));
    assert!(!requires_install_context(Platform::Local));
    assert!(!requires_install_context(Platform::Test));
}

#[test]
fn deployment_readiness_not_ready_blocks_with_check_codes() {
    let info = DeploymentInfoResponse {
        setup_config: None,
        readiness: Some(DeploymentReadiness {
            status: "notReady".to_string(),
            checks: vec![
                DeploymentReadinessCheck {
                    code: "MACHINE_BUNDLE_ARTIFACTS".to_string(),
                    status: "failed".to_string(),
                    message: "Machine bundle manifest is missing linux-x64.".to_string(),
                },
                DeploymentReadinessCheck {
                    code: "MANAGER_ACQUIRES_PLATFORM".to_string(),
                    status: "passed".to_string(),
                    message: "A machines-capable manager is ready.".to_string(),
                },
            ],
        }),
    };

    let error = validate_deployment_readiness(&info, Platform::Machines)
        .expect_err("notReady readiness must block the deploy");
    assert!(error.message.contains("machines deployments are not ready"));
    assert!(error.message.contains("MACHINE_BUNDLE_ARTIFACTS"));
    assert!(!error.message.contains("MANAGER_ACQUIRES_PLATFORM"));
}

#[test]
fn deployment_readiness_unknown_checks_do_not_block() {
    let info = DeploymentInfoResponse {
        setup_config: None,
        readiness: Some(DeploymentReadiness {
            status: "unknown".to_string(),
            checks: vec![DeploymentReadinessCheck {
                code: "MACHINES_HORIZON_CONTROL_PLANE_REACHABLE".to_string(),
                status: "unknown".to_string(),
                message: "Horizon control plane reachability could not be confirmed.".to_string(),
            }],
        }),
    };

    validate_deployment_readiness(&info, Platform::Machines)
        .expect("unknown readiness must not block the deploy");
}

#[test]
fn absent_readiness_does_not_block() {
    let info = DeploymentInfoResponse {
        setup_config: None,
        readiness: None,
    };

    validate_deployment_readiness(&info, Platform::Machines)
        .expect("absent readiness document must not block the deploy");
}

#[test]
fn machines_join_guidance_uses_install_script_when_available() {
    assert_eq!(
            machines_join_command(
                "acmectl",
                Some("https://packages.example.com/ws/prj/install.sh"),
                "aj1_wrapped-token"
            ),
            "curl -fsSL 'https://packages.example.com/ws/prj/install.sh' | sudo bash -s -- join --token 'aj1_wrapped-token'"
        );
}

#[test]
fn machines_join_guidance_shell_quotes_install_script_and_token() {
    assert_eq!(
            machines_join_command(
                "alien-deploy",
                Some("https://packages.example.com/ws/prj/install script.sh"),
                "aj1_token'with-quote"
            ),
            "curl -fsSL 'https://packages.example.com/ws/prj/install script.sh' | sudo bash -s -- join --token 'aj1_token'\"'\"'with-quote'"
        );
}

#[test]
fn machines_join_guidance_falls_back_to_installed_cli_without_install_script() {
    assert_eq!(
        machines_join_command("acmectl", None, "aj1_wrapped-token"),
        "sudo acmectl join --token 'aj1_wrapped-token'"
    );
}

#[test]
fn machines_join_token_response_preserves_wrapped_token() {
    let token = normalize_machines_join_token_response(MachinesJoinTokenResponse {
        join_token: " aj1_wrapped-token ".to_string(),
        control_plane_url: Some("https://horizon.example.com".to_string()),
        cluster_id: Some("cluster-123".to_string()),
    })
    .expect("wrapped token should be accepted");

    assert_eq!(token, "aj1_wrapped-token");
}

#[test]
fn machines_join_token_response_wraps_raw_token_with_context() {
    let token = normalize_machines_join_token_response(MachinesJoinTokenResponse {
        join_token: " hj_secret ".to_string(),
        control_plane_url: Some(" https://horizon.example.com ".to_string()),
        cluster_id: Some(" cluster-123 ".to_string()),
    })
    .expect("raw token with context should be wrapped");

    assert!(token.starts_with("aj1_"));
    let payload = URL_SAFE_NO_PAD
        .decode(token.trim_start_matches("aj1_"))
        .expect("wrapped token should be base64url");
    let payload: serde_json::Value =
        serde_json::from_slice(&payload).expect("wrapped token should be JSON");
    assert_eq!(payload["joinToken"], "hj_secret");
    assert_eq!(payload["controlPlaneUrl"], "https://horizon.example.com");
    assert_eq!(payload["clusterId"], "cluster-123");
}

#[test]
fn machines_join_token_response_rejects_raw_token_without_context() {
    let error = normalize_machines_join_token_response(MachinesJoinTokenResponse {
        join_token: "hj_secret".to_string(),
        control_plane_url: None,
        cluster_id: Some("cluster-123".to_string()),
    })
    .expect_err("raw token should require context");

    assert_eq!(error.code, "CONFIGURATION_ERROR");
    assert!(error.message.contains("without control plane context"));
}

#[test]
fn machines_deploy_prints_only_join_command() {
    assert!(!should_print_deploy_progress(Platform::Machines));
}

#[test]
fn machines_push_setup_suppresses_neutral_completion_message() {
    assert!(!should_print_push_setup_neutral_completion(
        Platform::Machines
    ));
    assert!(should_print_push_setup_neutral_completion(Platform::Aws));
}

#[test]
fn machines_push_setup_does_not_collect_cloud_environment() {
    assert!(!should_collect_push_setup_environment_info(
        Platform::Machines
    ));
    assert!(should_collect_push_setup_environment_info(Platform::Aws));
    assert!(should_collect_push_setup_environment_info(
        Platform::Kubernetes
    ));
}

#[test]
fn non_machines_deploy_prints_progress() {
    assert!(should_print_deploy_progress(Platform::Aws));
    assert!(should_print_deploy_progress(Platform::Local));
}

#[test]
fn load_stack_settings_omits_server_owned_deployment_model() {
    let args = UpArgs::parse_from(["alien-deploy", "--platform", "machines"]);
    let settings =
        load_stack_settings(&args, Platform::Machines, None).expect("settings should load");

    let wire = serde_json::to_value(settings).expect("settings should serialize");
    assert_eq!(wire.get("deploymentModel"), None);
}

#[test]
fn load_stack_settings_requests_pull_for_local() {
    let args = UpArgs::parse_from(["alien-deploy", "--platform", "local"]);
    let settings = load_stack_settings(&args, Platform::Local, None).expect("settings should load");

    assert_eq!(settings.deployment_model, DeploymentModel::Pull);
    let wire = serde_json::to_value(sdk_stack_settings(&settings).expect("sdk settings"))
        .expect("settings should serialize");
    assert_eq!(
        wire.get("deploymentModel"),
        Some(&serde_json::json!("pull"))
    );
}

#[test]
fn sdk_stack_settings_serializes_explicit_deployment_model() {
    let settings = StackSettings {
        deployment_model: DeploymentModel::Pull,
        ..StackSettings::default()
    };

    let wire = serde_json::to_value(
        sdk_stack_settings(&settings).expect("settings should convert to SDK type"),
    )
    .expect("settings should serialize");

    assert_eq!(
        wire.get("deploymentModel"),
        Some(&serde_json::json!("pull"))
    );
}

#[test]
fn local_tracking_uses_service_data_dir_by_default() {
    let args = UpArgs::parse_from(["alien-deploy", "--platform", "local"]);
    let local = local_tracking_metadata(&args, Platform::Local)
        .expect("local deployments should be tracked with local metadata");

    assert_eq!(
        local.data_dir,
        crate::commands::operator::default_service_data_dir()
    );
    assert!(local.service_managed);
}

#[test]
fn local_tracking_uses_foreground_data_dir() {
    let args = UpArgs::parse_from([
        "alien-deploy",
        "--platform",
        "local",
        "--foreground",
        "--data-dir",
        "/tmp/alien-foreground-state",
    ]);
    let local = local_tracking_metadata(&args, Platform::Local)
        .expect("local deployments should be tracked with local metadata");

    assert_eq!(local.data_dir, "/tmp/alien-foreground-state");
    assert!(!local.service_managed);
}

#[test]
fn stack_settings_external_bindings_are_copied_to_deployment_config() {
    let mut external_bindings = alien_core::ExternalBindings::new();
    external_bindings.insert(
        "storage",
        alien_core::ExternalBinding::Storage(alien_core::StorageBinding::s3("test-bucket")),
    );
    let stack_settings = StackSettings {
        external_bindings: Some(external_bindings),
        ..StackSettings::default()
    };
    let mut config = DeploymentConfig::builder()
        .stack_settings(stack_settings.clone())
        .environment_variables(alien_core::EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(alien_core::ExternalBindings::default())
        .allow_frozen_changes(false)
        .build();

    assert!(!config.external_bindings.has("storage"));
    apply_external_bindings_from_stack_settings(&mut config, &stack_settings);

    assert!(config.external_bindings.has("storage"));
}

#[test]
fn deploy_config_file_accepts_public_endpoints() {
    let mut file = tempfile::NamedTempFile::new().expect("temp config should be created");
    writeln!(
        file,
        r#"
	name = "local-gateway"
	platform = "local"

	[publicEndpoints.gateway]
	api = "https://gateway.example.test"
	"#
    )
    .expect("config should be written");

    let args = UpArgs::parse_from([
        "alien-deploy",
        "--config",
        file.path().to_str().expect("temp path should be UTF-8"),
    ]);

    let config = load_deploy_config(&args)
        .expect("config should load")
        .expect("config should exist");

    assert_eq!(config.name.as_deref(), Some("local-gateway"));
    assert_eq!(
        config
            .public_endpoints
            .as_ref()
            .and_then(|resources| resources.get("gateway"))
            .and_then(|endpoints| endpoints.get("api"))
            .map(String::as_str),
        Some("https://gateway.example.test")
    );
}

#[test]
fn public_endpoint_flag_overrides_config_public_endpoint() {
    let mut file = tempfile::NamedTempFile::new().expect("temp config should be created");
    writeln!(
        file,
        r#"
platform = "local"

[publicEndpoints.gateway]
api = "https://old.example.test"
"#
    )
    .expect("config should be written");

    let args = UpArgs::parse_from([
        "alien-deploy",
        "--config",
        file.path().to_str().expect("temp path should be UTF-8"),
        "--public-endpoint",
        "gateway.api=https://new.example.test",
    ]);
    let config = load_deploy_config(&args)
        .expect("config should load")
        .expect("config should exist");
    let public_endpoints = load_public_endpoints(&args, Platform::Local, Some(&config))
        .expect("public endpoints should load")
        .expect("public endpoints should exist");

    assert_eq!(
        public_endpoints
            .get("gateway")
            .and_then(|endpoints| endpoints.get("api"))
            .map(String::as_str),
        Some("https://new.example.test")
    );
}

#[test]
fn public_endpoint_flag_rejects_cloud_platforms() {
    let args = UpArgs::parse_from([
        "alien-deploy",
        "--platform",
        "aws",
        "--public-endpoint",
        "gateway.api=https://gateway.example.test",
    ]);

    let error =
        load_public_endpoints(&args, Platform::Aws, None).expect_err("aws should be rejected");
    assert_eq!(error.code, "VALIDATION_ERROR");
}

#[test]
fn public_endpoint_flag_accepts_machines_platform() {
    let args = UpArgs::parse_from([
        "alien-deploy",
        "--platform",
        "machines",
        "--public-endpoint",
        "gateway.api=https://gateway.example.test",
    ]);

    let public_endpoints = load_public_endpoints(&args, Platform::Machines, None)
        .expect("machines should accept external public endpoints")
        .expect("public endpoints should exist");

    assert_eq!(
        public_endpoints
            .get("gateway")
            .and_then(|endpoints| endpoints.get("api"))
            .map(String::as_str),
        Some("https://gateway.example.test")
    );
}

#[test]
fn public_endpoint_names_must_be_declared() {
    let daemon = alien_core::Daemon::new("gateway".to_string())
        .code(alien_core::DaemonCode::Image {
            image: "gateway:latest".to_string(),
        })
        .permissions("default".to_string())
        .public_endpoint(alien_core::PublicEndpoint {
            name: "api".to_string(),
            port: 8080,
            protocol: alien_core::ExposeProtocol::Http,
            host_label: None,
            wildcard_subdomains: false,
        })
        .build();
    let stack = Stack::new("test".to_string())
        .add(daemon, alien_core::ResourceLifecycle::Live)
        .build();

    let valid = HashMap::from([(
        "gateway".to_string(),
        HashMap::from([(
            "api".to_string(),
            "https://gateway.example.test".to_string(),
        )]),
    )]);
    validate_public_endpoint_names(&valid, &stack).expect("gateway exposes a public endpoint");

    let invalid = HashMap::from([(
        "gateway".to_string(),
        HashMap::from([(
            "missing".to_string(),
            "https://missing.example.test".to_string(),
        )]),
    )]);
    let error =
        validate_public_endpoint_names(&invalid, &stack).expect_err("missing endpoint should fail");
    assert_eq!(error.code, "VALIDATION_ERROR");
}

#[test]
fn deploy_config_file_accepts_compute_pool_selection() {
    let mut file = tempfile::NamedTempFile::new().expect("temp config should be created");
    writeln!(
        file,
        r#"
name = "cloud-runtime"
platform = "aws"

[compute.pools.general]
mode = "autoscale"
min = 2
max = 5
machine = "m8i.xlarge"
"#
    )
    .expect("config should be written");

    let args = UpArgs::parse_from([
        "alien-deploy",
        "--config",
        file.path().to_str().expect("temp path should be UTF-8"),
    ]);

    let config = load_deploy_config(&args)
        .expect("config should load")
        .expect("config should exist");
    let settings = load_stack_settings(&args, Platform::Aws, Some(&config))
        .expect("stack settings should load");
    let selection = settings
        .compute
        .as_ref()
        .and_then(|compute| compute.pools.get("general"))
        .expect("general compute pool should be configured");

    assert_eq!(selection.machine(), Some("m8i.xlarge"));
    assert_eq!(selection.min_size(), 2);
    assert_eq!(selection.max_size(), 5);
}

#[test]
fn parses_cloud_base_platform_for_kubernetes() {
    assert_eq!(
        parse_base_platform(Platform::Kubernetes, Some("aws")).unwrap(),
        Some(Platform::Aws)
    );
}

#[test]
fn rejects_base_platform_without_kubernetes_runtime() {
    assert!(parse_base_platform(Platform::Aws, Some("gcp")).is_err());
}

#[test]
fn rejects_non_cloud_base_platform_for_kubernetes() {
    assert!(parse_base_platform(Platform::Kubernetes, Some("local")).is_err());
}

#[test]
fn release_stack_for_kubernetes_uses_runtime_platform_not_base_platform() {
    let stack = alien_manager_api::types::StackByPlatform {
        aws: Some(serde_json::json!({ "id": "aws-stack" })),
        gcp: Some(serde_json::json!({ "id": "gcp-stack" })),
        azure: Some(serde_json::json!({ "id": "azure-stack" })),
        kubernetes: Some(serde_json::json!({ "id": "kubernetes-stack" })),
        machines: None,
        local: None,
        test: None,
    };

    let selected = release_stack_value_for_platform(stack, Platform::Kubernetes).unwrap();
    assert_eq!(selected["id"], "kubernetes-stack");
}

#[test]
fn split_image_tag_defaults_missing_tag_to_latest() {
    assert_eq!(
        split_image_tag("ghcr.io/alienplatform/alien-operator").unwrap(),
        (
            "ghcr.io/alienplatform/alien-operator".to_string(),
            "latest".to_string()
        )
    );
}

#[test]
fn split_image_tag_preserves_registry_port() {
    assert_eq!(
        split_image_tag("localhost:5000/alien-operator:v1").unwrap(),
        (
            "localhost:5000/alien-operator".to_string(),
            "v1".to_string()
        )
    );
}

#[test]
fn sanitize_kubernetes_dns_label_falls_back_when_empty() {
    assert_eq!(sanitize_kubernetes_dns_label("___"), "alien");
}

#[test]
fn resolve_token_reads_token_file() {
    let mut token_file = tempfile::NamedTempFile::new().expect("token file");
    token_file
        .write_all(b" ax_dg_file_token\n")
        .expect("write token");

    let cli = crate::Cli::try_parse_from([
        "alien-deploy",
        "deploy",
        "--token-file",
        token_file.path().to_str().expect("utf8 path"),
        "--platform",
        "local",
    ])
    .expect("parse deploy");
    let crate::Commands::Deploy(args) = cli.command else {
        panic!("expected deploy variant");
    };

    let token = resolve_token(&args, None).expect("token should resolve");
    assert_eq!(token, "ax_dg_file_token");
}

#[test]
fn resolve_token_rejects_empty_token_file() {
    let token_file = tempfile::NamedTempFile::new().expect("token file");

    let cli = crate::Cli::try_parse_from([
        "alien-deploy",
        "deploy",
        "--token-file",
        token_file.path().to_str().expect("utf8 path"),
        "--platform",
        "local",
    ])
    .expect("parse deploy");
    let crate::Commands::Deploy(args) = cli.command else {
        panic!("expected deploy variant");
    };

    let error = resolve_token(&args, None).expect_err("empty token file should fail");
    assert_eq!(error.code, "VALIDATION_ERROR");
}

fn stack_input(id: &str, kind: StackInputKind, required: bool) -> StackInputDefinition {
    StackInputDefinition {
        id: id.to_string(),
        kind,
        provided_by: vec![StackInputProvider::Deployer],
        required,
        label: id.to_string(),
        description: "Test input".to_string(),
        placeholder: None,
        default: None,
        platforms: None,
        validation: None,
        env: vec![],
    }
}

#[test]
fn deploy_config_file_accepts_stack_inputs() {
    let mut file = tempfile::NamedTempFile::new().expect("temp config should be created");
    writeln!(
        file,
        r#"
platform = "local"

[inputs]
region = "us-east-1"

[secretInputs]
apiKey = "secret-value"
"#
    )
    .expect("config should be written");

    let args = UpArgs::parse_from([
        "alien-deploy",
        "--config",
        file.path().to_str().expect("temp path should be UTF-8"),
    ]);
    let config = load_deploy_config(&args)
        .expect("config should load")
        .expect("config should exist");
    let values = collect_deployer_input_values(
        &[
            stack_input("region", StackInputKind::String, true),
            stack_input("apiKey", StackInputKind::Secret, true),
        ],
        &[],
        &[],
        Some(&config),
    )
    .expect("input values should parse");

    assert_eq!(values.get("region"), Some(&serde_json::json!("us-east-1")));
    assert_eq!(
        values.get("apiKey"),
        Some(&serde_json::json!("secret-value"))
    );
}

#[test]
fn stack_input_flags_override_config_values() {
    let mut file = tempfile::NamedTempFile::new().expect("temp config should be created");
    writeln!(
        file,
        r#"
platform = "local"

[inputs]
region = "old"
"#
    )
    .expect("config should be written");

    let args = UpArgs::parse_from([
        "alien-deploy",
        "--config",
        file.path().to_str().expect("temp path should be UTF-8"),
        "--input",
        "region=new",
    ]);
    let config = load_deploy_config(&args)
        .expect("config should load")
        .expect("config should exist");
    let values = collect_deployer_input_values(
        &[stack_input("region", StackInputKind::String, true)],
        &args.input_values,
        &args.secret_input_values,
        Some(&config),
    )
    .expect("input values should parse");

    assert_eq!(values.get("region"), Some(&serde_json::json!("new")));
}

#[test]
fn required_stack_inputs_fail_non_interactively() {
    let error = collect_deployer_input_values(
        &[stack_input("apiKey", StackInputKind::Secret, true)],
        &[],
        &[],
        None,
    )
    .expect_err("missing required input should fail");

    assert_eq!(error.code, "VALIDATION_ERROR");
    assert!(error.message.contains("Missing deployer input"));
}

#[test]
fn stack_input_values_are_typed() {
    let values = collect_deployer_input_values(
        &[
            stack_input("replicas", StackInputKind::Integer, true),
            stack_input("enabled", StackInputKind::Boolean, true),
            stack_input("hosts", StackInputKind::StringList, true),
        ],
        &[
            "replicas=3".to_string(),
            "enabled=true".to_string(),
            "hosts=a.example.com,b.example.com".to_string(),
        ],
        &[],
        None,
    )
    .expect("input values should parse");

    assert_eq!(values.get("replicas"), Some(&serde_json::json!(3)));
    assert_eq!(values.get("enabled"), Some(&serde_json::json!(true)));
    assert_eq!(
        values.get("hosts"),
        Some(&serde_json::json!(["a.example.com", "b.example.com"]))
    );
}
