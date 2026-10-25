//! Rendered uninstall behavior, including a report Pod that cannot start.

#![cfg(unix)]

use alien_core::{Stack, StackSettings};
use alien_helm::{generate_helm_chart, HelmChart, HelmOptions, HelmRegistry};
use serde::Deserialize;
use serde_json::Value;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};

#[allow(dead_code)]
#[path = "../src/test_utils.rs"]
mod test_utils;

fn chart() -> HelmChart {
    generate_helm_chart(
        &Stack::new("uninstall-test".to_string()).build(),
        HelmOptions {
            registry: &HelmRegistry::built_in(),
            stack_settings: StackSettings::default(),
            chart_name: "uninstall-test".to_string(),
        },
    )
    .expect("generate chart")
}

fn render(chart: &HelmChart, extra_values: &str) -> Vec<Value> {
    let values = format!(
        "management:\n  token: ax_test\n  name: uninstall-test\n  url: https://manager.example.test\nruntime:\n  encryption:\n    key: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n{extra_values}"
    );
    let output = test_utils::helm_template(&chart.files, Some(&values));
    output.assert_ok("render uninstall configuration");
    serde_yaml::Deserializer::from_str(&output.stdout)
        .map(|document| Value::deserialize(document).expect("rendered YAML document"))
        .collect()
}

fn cleanup_script(documents: &[Value]) -> &str {
    documents
        .iter()
        .find(|document| {
            document["kind"] == "Job"
                && document["metadata"]["name"]
                    .as_str()
                    .is_some_and(|name| name.ends_with("-cleanup"))
        })
        .expect("cleanup Job")
        .pointer("/spec/template/spec/containers/0/command/2")
        .and_then(Value::as_str)
        .expect("cleanup shell script")
}

fn report_job(script: &str) -> Value {
    let manifest = script
        .split_once("<<'UNINSTALL_REPORT'\n")
        .expect("optional report manifest")
        .1
        .split_once("\nUNINSTALL_REPORT\n")
        .expect("end of report manifest")
        .0;
    serde_yaml::from_str(manifest).expect("report Job YAML")
}

fn selects(selector: &Value, labels: &Value) -> bool {
    selector
        .as_object()
        .expect("match-label selector")
        .iter()
        .all(|(key, value)| labels.get(key) == Some(value))
}

#[test]
fn report_has_outbound_policy_without_joining_the_operator_service() {
    let documents = render(&chart(), "  networkPolicy:\n    enabled: true\n");
    let report = report_job(cleanup_script(&documents));
    let labels = &report["spec"]["template"]["metadata"]["labels"];
    let policy = documents
        .iter()
        .find(|document| {
            document["kind"] == "NetworkPolicy"
                && selects(&document["spec"]["podSelector"]["matchLabels"], labels)
        })
        .expect("an outbound policy selects the report Pod");
    assert_eq!(policy["spec"]["policyTypes"], serde_json::json!(["Egress"]));
    assert_eq!(policy["spec"]["egress"], serde_json::json!([{}]));
    for service in documents
        .iter()
        .filter(|document| document["kind"] == "Service")
    {
        assert!(
            !selects(&service["spec"]["selector"], labels),
            "report Pod must not receive Operator traffic: {service}"
        );
    }
    assert!(report["metadata"]["annotations"]["helm.sh/hook"].is_null());
    assert!(report["metadata"]["generateName"].as_str().is_some());
    assert_eq!(report["spec"]["ttlSecondsAfterFinished"], 120);
}

#[test]
fn report_start_wait_and_removal_failures_do_not_fail_cleanup() {
    let documents = render(&chart(), "");
    let script = cleanup_script(&documents);
    let optional = script
        .split_once("# Reporting is optional.")
        .expect("optional report block")
        .1;
    let directory = tempfile::tempdir().expect("temporary commands");
    let kubectl = directory.path().join("kubectl");
    fs::write(
        &kubectl,
        r#"#!/bin/sh
case " $* " in
  *" create "*) cat >/dev/null; echo job.batch/report-created; exit "$CREATE_STATUS" ;;
  *" wait "*) exit "$WAIT_STATUS" ;;
  *" delete "*) echo removed >>"$CALLS"; exit "$DELETE_STATUS" ;;
  *) exit 2 ;;
esac
"#,
    )
    .expect("write kubectl fixture");
    fs::set_permissions(&kubectl, fs::Permissions::from_mode(0o755))
        .expect("executable kubectl fixture");
    let timeout = directory.path().join("timeout");
    fs::write(&timeout, "#!/bin/sh\nshift\nexec \"$@\"\n").expect("write process timeout fixture");
    fs::set_permissions(&timeout, fs::Permissions::from_mode(0o755))
        .expect("executable process timeout fixture");
    let calls = directory.path().join("calls");
    for (create, wait, delete) in [(1, 0, 0), (0, 1, 0), (0, 0, 1), (0, 0, 0)] {
        let output = Command::new("sh")
            .args([
                "-ec",
                &format!("namespace=test\n# Reporting is optional.{optional}"),
            ])
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", directory.path().display()),
            )
            .env("CREATE_STATUS", create.to_string())
            .env("WAIT_STATUS", wait.to_string())
            .env("DELETE_STATUS", delete.to_string())
            .env("CALLS", &calls)
            .output()
            .expect("run actual optional report script");
        assert!(
            output.status.success(),
            "report failure stopped cleanup: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(
        fs::read_to_string(calls)
            .expect("report deletions")
            .lines()
            .count(),
        3
    );
}
