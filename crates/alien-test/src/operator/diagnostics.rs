use std::time::Duration;

use super::{apply_command_env, kubectl};

/// Capture the Kubernetes state that explains a failed `helm install --wait`
/// before the caller tears the namespace down. This intentionally avoids Helm
/// values and Secret objects so credentials cannot be printed into CI logs.
pub(super) async fn collect_helm_failure_diagnostics(
    release_name: &str,
    namespace: &str,
    kubeconfig: Option<&str>,
    kube_context: Option<&str>,
    command_env: &[(String, String)],
) -> String {
    let selector = format!("app.kubernetes.io/instance={release_name}");
    let commands = [
        (
            "workloads",
            vec![
                "-n".to_string(),
                namespace.to_string(),
                "get".to_string(),
                "deployments,replicasets,pods,persistentvolumeclaims,services".to_string(),
                "-o".to_string(),
                "wide".to_string(),
            ],
        ),
        (
            "deployment and pod details",
            vec![
                "-n".to_string(),
                namespace.to_string(),
                "describe".to_string(),
                "deployments,pods".to_string(),
                "-l".to_string(),
                selector.clone(),
            ],
        ),
        (
            "events",
            vec![
                "-n".to_string(),
                namespace.to_string(),
                "get".to_string(),
                "events".to_string(),
                "--sort-by=.lastTimestamp".to_string(),
            ],
        ),
        (
            "container logs",
            vec![
                "-n".to_string(),
                namespace.to_string(),
                "logs".to_string(),
                "-l".to_string(),
                selector,
                "--all-containers=true".to_string(),
                "--tail=200".to_string(),
                "--prefix=true".to_string(),
            ],
        ),
        (
            "previous container logs",
            vec![
                "-n".to_string(),
                namespace.to_string(),
                "logs".to_string(),
                "-l".to_string(),
                format!("app.kubernetes.io/instance={release_name}"),
                "--all-containers=true".to_string(),
                "--previous=true".to_string(),
                "--tail=200".to_string(),
                "--prefix=true".to_string(),
            ],
        ),
    ];

    let mut diagnostics = String::from("=== Kubernetes diagnostics ===\n");
    for (label, args) in commands {
        let mut cmd = kubectl(kubeconfig, kube_context);
        cmd.arg("--request-timeout=10s");
        cmd.args(&args);
        apply_command_env(&mut cmd, command_env);
        cmd.kill_on_drop(true);
        diagnostics.push_str(&format!("--- {label} ---\n"));
        match tokio::time::timeout(Duration::from_secs(15), cmd.output()).await {
            Ok(Ok(output)) => {
                diagnostics.push_str(&String::from_utf8_lossy(&output.stdout));
                diagnostics.push_str(&String::from_utf8_lossy(&output.stderr));
            }
            Ok(Err(error)) => {
                diagnostics.push_str(&format!("failed to run kubectl: {error}\n"));
            }
            Err(_) => diagnostics.push_str("kubectl diagnostics timed out after 15s\n"),
        }
    }
    diagnostics
}
