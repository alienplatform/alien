//! Postgres heartbeat permission set, checked by rendering it for a stack and asking whether
//! concrete calls are allowed under the IAM evaluation model in `iam_policy`.
mod common;
mod iam_policy;

use alien_permissions::{
    generators::AwsRuntimePermissionsGenerator, get_permission_set, BindingTarget,
    PermissionContext,
};
use iam_policy::{evaluate, Decision, Request};

const RDS: &str = "arn:aws:rds:us-east-1:123456789012";

fn heartbeat(target: BindingTarget) -> Vec<alien_permissions::generators::AwsIamStatement> {
    let context = PermissionContext::new()
        .with_stack_prefix("my-stack")
        .with_resource_name("db")
        .with_aws_account_id("123456789012")
        .with_aws_region("us-east-1");
    AwsRuntimePermissionsGenerator::new()
        .generate_policy(
            get_permission_set("postgres/heartbeat").unwrap(),
            target,
            &context,
        )
        .unwrap()
        .statement
}

/// An identity holding only `postgres/heartbeat` can read the capacity metric (CloudWatch
/// accepts only `Resource: "*"` for it) and still reads only its own cluster and instances.
#[test]
fn aws_heartbeat_reads_metrics_and_only_its_own_rds_objects() {
    for target in [BindingTarget::Stack, BindingTarget::Resource] {
        let statements = heartbeat(target);
        let allowed = |r: Request| evaluate(&statements, &r) == Decision::Allowed;
        assert!(
            allowed(Request::new("cloudwatch:GetMetricData", "*")),
            "{target:?}"
        );
        assert!(allowed(Request::new(
            "rds:DescribeDBClusters",
            format!("{RDS}:cluster:my-stack-db")
        )));
        assert!(allowed(Request::new(
            "rds:DescribeDBInstances",
            format!("{RDS}:db:my-stack-db-1")
        )));
        assert!(!allowed(Request::new(
            "rds:DescribeDBClusters",
            format!("{RDS}:cluster:other-stack-db")
        )));
    }
}
