//! Opt-in AWS certificate renewal probe. The caller owns the image, role, domain material,
//! resource ledger, and teardown. No cloud resources are touched without an explicit input file.
use alien_aws_clients::{AwsClientConfig, AwsClientConfigExt};
use alien_core::{
    ClientConfig, DomainMetadata, Platform, ResourceStatus, ServiceAccount, StackState, Worker,
    WorkerCode, WorkerPublicEndpoint,
};
use alien_infra::{
    controller_test::SingleControllerExecutor, AwsServiceAccountController, AwsWorkerController,
    StackExecutor, StackResourceStateExt,
};
use serde::Deserialize;
use std::{path::PathBuf, time::Duration};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Probe {
    resource_prefix: String,
    image: String,
    role_arn: String,
    domain_metadata: DomainMetadata,
    state_path: PathBuf,
    mode: String,
    #[serde(default)]
    expect_failure: bool,
}

#[tokio::test]
#[ignore = "requires an explicitly provisioned AWS test image, role, and certificate input file"]
async fn live_aws_worker_certificate_renewal() {
    let path = std::env::var("ALIEN_CERTIFICATE_TEST_INPUT").expect("explicit probe input");
    let probe: Probe = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(probe.resource_prefix.starts_with("c1619-"));
    let client = ClientConfig::Aws(Box::new(AwsClientConfig::from_std_env().await.unwrap()));
    let worker = Worker::new("w".to_string())
        .code(WorkerCode::Image { image: probe.image })
        .permissions("default-profile".to_string())
        .public_endpoint(WorkerPublicEndpoint {
            name: "api".to_string(),
            host_label: None,
            wildcard_subdomains: false,
        })
        .build();
    let mut role =
        AwsServiceAccountController::mock_ready(&format!("{}-execution", probe.resource_prefix));
    role.role_arn = Some(probe.role_arn);
    let saved: Option<StackState> = if probe.state_path.exists() {
        Some(serde_json::from_slice(&std::fs::read(&probe.state_path).unwrap()).unwrap())
    } else {
        None
    };
    let controller = saved
        .as_ref()
        .map(|state| {
            let controller = state.resources["w"]
                .get_internal_controller()
                .unwrap()
                .unwrap();
            controller
                .as_any()
                .downcast_ref::<AwsWorkerController>()
                .unwrap()
                .clone()
        })
        .unwrap_or_default();
    let mut single = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(controller)
        .platform(Platform::Aws)
        .client_config(client.clone())
        .resource_prefix(probe.resource_prefix)
        .domain_metadata(probe.domain_metadata)
        .with_dependency(
            ServiceAccount::new("default-profile-sa".to_string()).build(),
            role,
        )
        .build()
        .await
        .unwrap();
    if probe.mode == "create" || probe.mode == "delete" {
        if probe.mode == "delete" {
            single.delete().unwrap();
        }
        for _ in 0..120 {
            let result = single.step().await;
            std::fs::write(
                &probe.state_path,
                serde_json::to_vec_pretty(single.stack_state()).unwrap(),
            )
            .unwrap();
            let result = result.expect("live controller step");
            println!("resource status: {:?}", single.status());
            if matches!(
                single.status(),
                ResourceStatus::Running | ResourceStatus::Deleted
            ) {
                return;
            }
            tokio::time::sleep(result.suggested_delay.unwrap_or(Duration::from_millis(100))).await;
        }
        panic!("live controller did not converge");
    }
    assert!(matches!(probe.mode.as_str(), "renew" | "retry"));
    let (stack, config) =
        single.with_context(|ctx| (ctx.desired_stack.clone(), ctx.deployment_config.clone()));
    let executor = StackExecutor::builder(&stack, client)
        .deployment_config(&config)
        .build()
        .unwrap();
    let mut state = saved.expect("renewal requires an existing deployment");
    assert!(state.resources["w"].previous_config.is_none());
    if probe.mode == "retry" {
        assert!(state
            .resources
            .get_mut("w")
            .unwrap()
            .retry_failed()
            .unwrap());
    }
    for _ in 0..10 {
        let result = executor.refresh(state).await.unwrap();
        state = result.next_state;
        std::fs::write(
            &probe.state_path,
            serde_json::to_vec_pretty(&state).unwrap(),
        )
        .unwrap();
        let worker = &state.resources["w"];
        println!(
            "resource status: {:?}; delay: {:?}",
            worker.status, result.suggested_delay_ms
        );
        if let Some(error) = &worker.error {
            println!("{}: {}", error.code, error.message);
        }
        if worker.status == ResourceStatus::UpdateFailed {
            assert!(probe.expect_failure, "renewal failed");
            assert!(worker
                .error
                .as_ref()
                .unwrap()
                .message
                .contains("No previous configuration available"));
            return;
        }
        if worker.status == ResourceStatus::Running {
            assert!(!probe.expect_failure, "baseline unexpectedly succeeded");
            assert!(worker.error.is_none());
            assert!(worker.last_failed_state.is_none());
            return;
        }
        assert!(!matches!(
            worker.status,
            ResourceStatus::RefreshFailed | ResourceStatus::ProvisionFailed
        ));
    }
    panic!("renewal did not converge");
}
