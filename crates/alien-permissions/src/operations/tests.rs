use serde_json::{json, Value};

use super::request_json;

fn run(request: Value) -> Value {
    serde_json::from_str(&request_json(&request.to_string())).unwrap()
}

fn aws_statement(actions: &[&str], resources: &[&str]) -> Value {
    json!({"effect":"Allow", "actions":actions, "resources":resources, "condition":null,
        "reasons":["objects/read: Read objects"], "sources":[{"plugin":"objects","operation":"read","reason":"Read objects"}]})
}

#[test]
fn bounds_bucket_and_queue_grants_and_rejects_missing_ceilings() {
    let statements = vec![
        aws_statement(&["s3:GetObject"], &["arn:aws:s3:::*/*"]),
        aws_statement(&["sqs:GetQueueAttributes"], &["arn:aws:sqs:*:*:*"]),
    ];
    let result = run(
        json!({"task":"compileAws","statements":statements,"ceilings":{"s3BucketArns":["arn:aws:s3:::example-bucket"],"sqsQueueArns":["arn:aws:sqs:us-east-1:123456789012:example"]}}),
    );
    assert_eq!(result["ok"], true);
    assert_eq!(
        result["value"][0]["resources"],
        json!(["arn:aws:s3:::example-bucket/*"])
    );
    assert_eq!(
        result["value"][1]["resources"],
        json!(["arn:aws:sqs:us-east-1:123456789012:example"])
    );
    let missing = run(
        json!({"task":"compileAws","statements":statements,"ceilings":{"s3BucketArns":[],"sqsQueueArns":[]}}),
    );
    assert_eq!(missing["ok"], false);
    assert!(missing["error"]["message"]
        .as_str()
        .unwrap()
        .contains("s3BucketArns"));
}

#[test]
fn wildcard_actions_and_unknown_wildcard_resources_fail_closed() {
    for statement in [
        aws_statement(&["s3:*"], &["arn:aws:s3:::example/*"]),
        aws_statement(&["iam:PassRole"], &["*"]),
        aws_statement(
            &["iam:PassRole"],
            &["arn:aws:iam::123456789012:role/administrator"],
        ),
    ] {
        let result = run(
            json!({"task":"compileAws","statements":[statement],"ceilings":{"s3BucketArns":[],"sqsQueueArns":[]}}),
        );
        assert_eq!(result["ok"], false);
    }
}

#[test]
fn gcs_metadata_grants_stay_on_buckets() {
    let grant = json!({"permission":"storage.objects.list","scope":"projects/${projectName}/buckets/${resourceName}","sources":[]});
    let result = run(
        json!({"task":"compileGcp","grants":[grant],"ceilings":{"gcsBucketNames":["example-bucket","example-bucket"]}}),
    );
    assert_eq!(result["value"]["project"], json!([]));
    assert_eq!(
        result["value"]["buckets"]["names"],
        json!(["example-bucket"])
    );
    assert_eq!(result["value"]["buckets"]["grants"][0], grant);
    for names in [json!([]), json!(["gs://example-bucket"]), json!(["*"])] {
        assert_eq!(
            run(json!({"task":"compileGcp","grants":[grant],"ceilings":{"gcsBucketNames":names}}))
                ["ok"],
            false
        );
    }
}

#[test]
fn raw_gcp_grants_cannot_bypass_reviewed_permissions_or_bucket_scopes() {
    for names in [json!([]), json!(["example-bucket"])] {
        for permission in ["iam.serviceAccounts.getAccessToken", "storage.objects.list"] {
            let result = run(json!({"task":"compileGcp","grants":[{
                "permission":permission,"scope":"projects/${projectName}","sources":[]
            }],"ceilings":{"gcsBucketNames":names}}));
            assert_eq!(result["ok"], false);
            assert_eq!(result["error"]["code"], "OPERATION_PERMISSION_INVALID");
            assert!(result["error"]["message"]
                .as_str()
                .unwrap()
                .contains(permission));
        }
    }
}

#[test]
fn named_capability_matches_builtin_grant_but_custom_inline_aws_fails() {
    let inline = json!({"id":"operations/ec2/instances","description":"Inspect instances","platforms":{"aws":[{"effect":"Allow","grant":{"actions":["ec2:DescribeInstances"]},"binding":{"resource":{"resources":["*"]}}}]}});
    let plugin = |permissions: Value| json!({"name":"inspector","tier":"read-only","operations":[{"name":"inspect","permissions":permissions}]});
    let named =
        run(json!({"task":"resolvePlugin","plugin":plugin(json!(["operations/ec2/instances"]))}));
    let builtin =
        run(json!({"task":"resolvePlugin","origin":"builtin","plugin":plugin(json!([inline]))}));
    assert_eq!(named["ok"], true);
    assert_eq!(builtin["ok"], true);
    assert_eq!(
        named["value"]["operations"][0]["permissions"]["aws"][0]["actions"],
        builtin["value"]["operations"][0]["permissions"]["aws"][0]["actions"]
    );
    assert_eq!(
        run(json!({"task":"resolvePlugin","plugin":plugin(json!([inline]))}))["ok"],
        false
    );
    assert_eq!(
        run(json!({"task":"resolvePlugin","plugin":plugin(json!(["operations/ec2/admin"]))}))["ok"],
        false
    );
}

#[test]
fn kubernetes_reads_and_remediation_remain_distinct() {
    let request = |reference: &str, tier: &str| json!({"task":"resolveKubernetes","references":[reference],"tier":tier,"declaredPermissions":null});
    assert_eq!(
        run(request("pods/get", "read-only"))["value"]["rules"][0]["verbs"],
        json!(["get"])
    );
    assert_eq!(run(request("pods/delete", "read-only"))["ok"], false);
    assert_eq!(
        run(request("pods/delete", "mutating"))["value"]["rules"][0]["verbs"],
        json!(["delete"])
    );
    let invalid = json!({"schemaVersion":1,"rules":[{"apiGroup":"","resource":"secrets","verbs":["get"],"reason":"Read secret"}]});
    assert_eq!(
        run(json!({"task":"validateKubernetes","permissions":invalid,"tier":"read-only"}))["ok"],
        false
    );
}

#[test]
fn raw_kubernetes_grants_are_validated_before_mode_filtering() {
    let grant = |resource: &str, verbs: &[&str], names: &[&str]| json!({"apiGroup":"","resource":resource,"verbs":verbs,"resourceNames":names,"sources":[]});
    for (mode, verbs) in [
        ("diagnostics", json!(["get"])),
        ("remediation", json!(["get", "delete"])),
    ] {
        let valid = grant("pods", &["get", "delete"], &["selected"]);
        let result = run(json!({"task":"compileKubernetes","mode":mode,"grants":[valid]}));
        assert_eq!(result["ok"], true);
        assert_eq!(result["value"][0]["verbs"], verbs);
        assert_eq!(result["value"][0]["resourceNames"], json!(["selected"]));
        for invalid in [
            grant("secrets", &["get"], &[]),
            grant("pods/exec", &["create"], &[]),
            grant("pods", &["*"], &[]),
            grant("pods", &["delete"], &["*"]),
        ] {
            let result = run(json!({"task":"compileKubernetes","mode":mode,"grants":[invalid]}));
            assert_eq!(result["ok"], false);
            assert_eq!(result["error"]["code"], "OPERATION_PERMISSION_INVALID");
        }
    }
}

#[test]
fn compaction_preserves_partial_action_resource_overlaps() {
    let result = run(
        json!({"task":"compactAws","statements":[aws_statement(&["s3:GetObject"], &["a","b"]),aws_statement(&["s3:GetObjectTagging"], &["b"])]}),
    );
    assert_eq!(result["value"].as_array().unwrap().len(), 2);
    assert!(result["value"]
        .as_array()
        .unwrap()
        .iter()
        .all(|statement| !(statement["actions"]
            == json!(["s3:GetObject", "s3:GetObjectTagging"])
            && statement["resources"] == json!(["a", "b"]))));
}

#[test]
fn runtime_task_returns_the_complete_serialized_read_only_contract() {
    let result = run(json!({"task": "kubernetesOperatorRuntime"}));
    assert_eq!(result["ok"], true);
    let rules = result["value"].as_array().unwrap();
    assert_eq!(rules.len(), 6);
    for (rule, (group, resource)) in rules.iter().zip([
        ("apps", "deployments"),
        ("apps", "statefulsets"),
        ("apps", "daemonsets"),
        ("", "pods"),
        ("", "events"),
        ("metrics.k8s.io", "pods"),
    ]) {
        assert_eq!(rule["apiGroup"], group);
        assert_eq!(rule["resource"], resource);
        assert_eq!(rule["verbs"], json!(["list"]));
        assert_eq!(rule["resourceNames"], json!([]));
        assert!(!rule["reason"].as_str().unwrap().is_empty());
        assert_eq!(rule.as_object().unwrap().len(), 5);
    }
}
