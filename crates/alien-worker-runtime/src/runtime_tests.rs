use std::collections::HashMap;
use std::process::Command as StdCommand;
use std::sync::Arc;

use alien_core::{
    ENV_ALIEN_COMMANDS_TOKEN, ENV_ALIEN_CURRENT_WORKER_BINDING_NAME, ENV_ALIEN_DEPLOYMENT_ID,
    ENV_ALIEN_RUNTIME_SECRETS, ENV_ALIEN_TRANSPORT, ENV_ALIEN_WORKER_GRPC_ADDRESS,
};
use alien_worker_protocol::{ControlGrpcServer, WaitUntilGrpcServer};
use tokio::process::Command;
use tokio::sync::broadcast;

use super::{
    application_runtime_env, command_push_config, run_transport, runtime_only_env,
    start_application, RuntimeConfig, TransportType, ENV_ALIEN_BINDINGS_GRPC_ADDRESS,
    ENV_ALIEN_BINDINGS_MODE,
};

const SUBPROCESS_DRIVER: &str = "ALIEN_TEST_RUNTIME_ENV_SUBPROCESS_DRIVER";
const APPLICATION_OBSERVER: &str = "ALIEN_TEST_APPLICATION_ENV_OBSERVER";
const USER_VISIBLE_ENV: &str = "ALIEN_TEST_USER_VISIBLE_ENV";

#[cfg(unix)]
async fn unused_local_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("reserve local port");
    listener.local_addr().expect("local address").port()
}

#[cfg(unix)]
async fn wait_for_listener(port: u16) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async move {
        loop {
            if tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .is_ok()
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("transport listener did not start");
}

#[cfg(unix)]
async fn assert_port_released(port: u16) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async move {
        loop {
            if tokio::net::TcpListener::bind(("127.0.0.1", port))
                .await
                .is_ok()
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("transport listener remained alive after runtime exit");
}

#[cfg(unix)]
#[tokio::test]
async fn child_exit_stops_and_joins_transport_listener() {
    let port = unused_local_port().await;
    let config = RuntimeConfig::builder()
        .transport(TransportType::Local)
        .transport_port(port)
        .command(vec!["unused".to_string()])
        .build();
    let temp = tempfile::tempdir().expect("child exit marker directory");
    let exit_marker = temp.path().join("exit");
    let mut child = Command::new("/bin/sh")
        .args([
            "-c",
            "while [ ! -f \"$1\" ]; do sleep 0.01; done",
            "runtime-child",
            exit_marker.to_str().expect("UTF-8 marker path"),
        ])
        .spawn()
        .expect("spawn short-lived child");
    let (_shutdown_tx, shutdown_rx) = broadcast::channel(1);
    let listener_observer = tokio::spawn(async move {
        wait_for_listener(port).await;
        tokio::fs::write(exit_marker, b"exit")
            .await
            .expect("release child after listener starts");
    });

    run_transport(
        &config,
        Arc::new(ControlGrpcServer::new()),
        None,
        None,
        Arc::new(WaitUntilGrpcServer::new()),
        &mut child,
        shutdown_rx,
    )
    .await
    .expect("successful child exit should stop transport");
    listener_observer.await.expect("listener observer");
    assert_port_released(port).await;
}

#[cfg(unix)]
#[tokio::test]
async fn normal_shutdown_stops_listener_and_child_before_returning() {
    let port = unused_local_port().await;
    let config = RuntimeConfig::builder()
        .transport(TransportType::Local)
        .transport_port(port)
        .command(vec!["unused".to_string()])
        .build();
    let mut child = Command::new("/bin/sh")
        .args(["-c", "sleep 60"])
        .spawn()
        .expect("spawn long-lived child");
    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    let signal = tokio::spawn(async move {
        wait_for_listener(port).await;
        shutdown_tx.send(()).expect("runtime shutdown receiver");
    });

    run_transport(
        &config,
        Arc::new(ControlGrpcServer::new()),
        None,
        None,
        Arc::new(WaitUntilGrpcServer::new()),
        &mut child,
        shutdown_rx,
    )
    .await
    .expect("normal shutdown");
    signal.await.expect("shutdown signal task");
    assert_port_released(port).await;
    assert!(
        child.try_wait().expect("read child status").is_some(),
        "runtime returned while application child was still alive"
    );
}

#[test]
fn command_push_token_is_runtime_only() {
    assert!(runtime_only_env(ENV_ALIEN_COMMANDS_TOKEN));
    assert!(runtime_only_env(ENV_ALIEN_RUNTIME_SECRETS));
    assert!(!runtime_only_env("USER_SECRET"));
}

#[test]
fn command_push_requires_trusted_worker_identity() {
    let config = RuntimeConfig::builder()
        .transport(TransportType::Local)
        .command(vec!["app".to_string()])
        .env_vars(HashMap::from([(
            ENV_ALIEN_COMMANDS_TOKEN.to_string(),
            "deployment-token".to_string(),
        )]))
        .build();

    let Err(error) = command_push_config(&config, &HashMap::new()) else {
        panic!("a command token without Worker identity must fail closed");
    };
    assert_eq!(error.code, "CONFIGURATION_INVALID");
    assert_eq!(
        error
            .context
            .as_ref()
            .and_then(|context| context.get("field"))
            .and_then(|value| value.as_str()),
        Some(ENV_ALIEN_CURRENT_WORKER_BINDING_NAME)
    );

    let mut config = config;
    config.env_vars.insert(
        ENV_ALIEN_CURRENT_WORKER_BINDING_NAME.to_string(),
        "reports".to_string(),
    );
    let Err(error) = command_push_config(&config, &HashMap::new()) else {
        panic!("a command token without deployment identity must fail closed");
    };
    assert_eq!(error.code, "CONFIGURATION_INVALID");

    config.env_vars.insert(
        ENV_ALIEN_DEPLOYMENT_ID.to_string(),
        "deployment".to_string(),
    );
    let command_push = command_push_config(&config, &HashMap::new())
        .expect("valid command push config")
        .expect("command push enabled");
    assert_eq!(command_push.token, "deployment-token");
    assert_eq!(command_push.deployment_id, "deployment");
    assert_eq!(command_push.worker_resource_id, "reports");
}

#[test]
fn command_push_rejects_empty_or_whitespace_token() {
    let config = RuntimeConfig::builder()
        .transport(TransportType::Local)
        .command(vec!["app".to_string()])
        .env_vars(HashMap::from([(
            ENV_ALIEN_COMMANDS_TOKEN.to_string(),
            String::new(),
        )]))
        .build();

    let Err(error) = command_push_config(&config, &HashMap::new()) else {
        panic!("an empty command token must fail closed");
    };
    assert_eq!(error.code, "CONFIGURATION_INVALID");

    let whitespace_config = RuntimeConfig::builder()
        .transport(TransportType::Local)
        .command(vec!["app".to_string()])
        .env_vars(HashMap::from([(
            ENV_ALIEN_COMMANDS_TOKEN.to_string(),
            "  \t".to_string(),
        )]))
        .build();
    let Err(error) = command_push_config(&whitespace_config, &HashMap::new()) else {
        panic!("a whitespace-only command token must fail closed");
    };
    assert_eq!(error.code, "CONFIGURATION_INVALID");
}

#[test]
fn application_subprocess_cannot_inherit_runtime_only_credentials() {
    let output = StdCommand::new(std::env::current_exe().expect("current test executable"))
        .args([
            "--exact",
            "runtime::tests::runtime_only_environment_subprocess_driver",
            "--nocapture",
        ])
        .env(SUBPROCESS_DRIVER, "1")
        .env(ENV_ALIEN_COMMANDS_TOKEN, "must-not-reach-application")
        .env(
            ENV_ALIEN_RUNTIME_SECRETS,
            "must-not-reach-application-either",
        )
        .output()
        .expect("run isolated subprocess driver");

    assert!(
        output.status.success(),
        "subprocess environment regression failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn runtime_only_environment_subprocess_driver() {
    if std::env::var(SUBPROCESS_DRIVER).as_deref() != Ok("1") {
        return;
    }

    let executable = std::env::current_exe().expect("current test executable");
    let config = RuntimeConfig::builder()
        .transport(TransportType::Local)
        .command(vec![
            executable.to_string_lossy().to_string(),
            "--exact".to_string(),
            "runtime::tests::application_environment_observer".to_string(),
            "--nocapture".to_string(),
        ])
        .env_vars(HashMap::from([
            (APPLICATION_OBSERVER.to_string(), "1".to_string()),
            (USER_VISIBLE_ENV.to_string(), "visible".to_string()),
        ]))
        .build();

    let mut child = start_application(&config, &HashMap::new(), crate::config::LogExporter::None)
        .await
        .expect("start environment-observer application");
    let status = child.wait().await.expect("wait for observer application");
    assert!(status.success(), "environment observer rejected child env");
}

#[test]
fn application_environment_observer() {
    if std::env::var(APPLICATION_OBSERVER).as_deref() != Ok("1") {
        return;
    }

    assert_eq!(
        std::env::var(ENV_ALIEN_COMMANDS_TOKEN),
        Err(std::env::VarError::NotPresent)
    );
    assert_eq!(
        std::env::var(ENV_ALIEN_RUNTIME_SECRETS),
        Err(std::env::VarError::NotPresent)
    );
    assert_eq!(std::env::var(USER_VISIBLE_ENV).as_deref(), Ok("visible"));
}

#[test]
fn application_runtime_env_injects_both_worker_protocol_address_names() {
    for (transport, expected) in [
        (TransportType::Lambda, "lambda"),
        (TransportType::CloudRun, "cloud-run"),
        (TransportType::ContainerApp, "container-app"),
        (TransportType::Http, "http"),
        (TransportType::Local, "local"),
    ] {
        let config = RuntimeConfig::builder()
            .transport(transport)
            .transport_port(61000)
            .command(vec!["app".to_string()])
            .worker_grpc_address("127.0.0.1:60000".to_string())
            .build();

        let env = application_runtime_env(&config)
            .into_iter()
            .collect::<HashMap<_, _>>();

        assert_eq!(
            env.get(ENV_ALIEN_WORKER_GRPC_ADDRESS),
            Some(&"127.0.0.1:60000".to_string())
        );
        assert_eq!(
            env.get(ENV_ALIEN_BINDINGS_GRPC_ADDRESS),
            Some(&"127.0.0.1:60000".to_string())
        );
        assert_eq!(env.get(ENV_ALIEN_BINDINGS_MODE), Some(&"grpc".to_string()));
        assert_eq!(env.get(ENV_ALIEN_TRANSPORT), Some(&expected.to_string()));
        assert_eq!(env.get("PORT"), None);
    }
}
