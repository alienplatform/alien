//! Runs a Helm-owned ServiceAccount's Ready handler against a real cluster after
//! the account disappears (a rollback to a chart that no longer creates it).
//! Ignored: needs KIND_CONTEXT (a kubeconfig context) and `kubectl` on PATH.

use alien_core::{
    ClientConfig, DeploymentConfig, EnvironmentVariablesSnapshot, KubernetesClientConfig,
    Platform, ResourceLifecycle, ResourceStatus, ServiceAccount, Stack, StackState,
};
use alien_infra::StackExecutor;
use std::process::Command;

fn kubectl(context: &str, args: &[&str]) {
    let out = Command::new("kubectl")
        .args(["--context", context])
        .args(args)
        .output()
        .expect("kubectl runs");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

#[tokio::test]
#[ignore = "needs a kind cluster: KIND_CONTEXT"]
async fn a_missing_helm_service_account_surfaces_through_the_running_refresh() {
    let context = std::env::var("KIND_CONTEXT").expect("KIND_CONTEXT");
    let namespace = "alien-refresh-probe";
    let _ = Command::new("kubectl")
        .args(["--context", &context, "create", "namespace", namespace])
        .output();

    let stack = Stack::new("demo".to_string())
        .add(
            ServiceAccount::new("b-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let raw = Command::new("kubectl")
        .args(["--context", &context, "config", "view", "--raw", "--minify", "-o", "json"])
        .output()
        .expect("kubectl config view");
    let view: serde_json::Value = serde_json::from_slice(&raw.stdout).expect("kubeconfig json");
    let field = |value: &serde_json::Value| value.as_str().map(str::to_string);
    let client_config = ClientConfig::Kubernetes(Box::new(KubernetesClientConfig::Manual {
        server_url: field(&view["clusters"][0]["cluster"]["server"]).expect("server"),
        certificate_authority_data: field(
            &view["clusters"][0]["cluster"]["certificate-authority-data"],
        ),
        insecure_skip_tls_verify: None,
        client_certificate_data: field(&view["users"][0]["user"]["client-certificate-data"]),
        client_key_data: field(&view["users"][0]["user"]["client-key-data"]),
        token: None,
        username: None,
        password: None,
        namespace: Some(namespace.to_string()),
        additional_headers: Default::default(),
    }));
    let config = DeploymentConfig::builder()
        .stack_settings(Default::default())
        .allow_frozen_changes(false)
        .external_bindings(Default::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .build();
    let mut state = StackState::new(Platform::Kubernetes);
    let name = format!("{}-b-sa", state.resource_prefix);
    kubectl(&context, &["-n", namespace, "create", "serviceaccount", &name]);

    let executor = StackExecutor::builder(&stack, client_config.clone())
        .deployment_config(&config)
        .build()
        .expect("executor");
    for _ in 0..10 {
        state = executor.step(state).await.expect("step").next_state;
        if state.resources["b-sa"].status == ResourceStatus::Running {
            break;
        }
    }
    assert_eq!(
        state.resources["b-sa"].status,
        ResourceStatus::Running,
        "{:?}",
        state.resources["b-sa"].error
    );

    // The rollback removes the account.
    kubectl(&context, &["-n", namespace, "delete", "serviceaccount", &name]);
    for round in 1..=2 {
        let step = executor.refresh(state).await.expect("refresh");
        state = step.next_state;
        let resource = &state.resources["b-sa"];
        assert_eq!(
            resource.status,
            ResourceStatus::RefreshFailed,
            "refresh {round}: a failed check must surface, not leave the account Running"
        );
        println!(
            "refresh {round}: status={:?} retry_attempt={} error={}",
            resource.status,
            resource.retry_attempt,
            resource.error.as_ref().map(|e| format!("{} retryable={} source={:?}", e.message, e.retryable, e.source.as_ref().map(|s| (s.code.clone(), s.retryable)))).unwrap_or("-".into())
        );
    }
    // One failed health check is enough; a missing account is not transient.
    assert_eq!(
        state.resources["b-sa"].status,
        ResourceStatus::RefreshFailed
    );
}
