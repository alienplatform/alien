//! Render the optional CloudWatch metrics policy for review or IAM simulation.
//! This example never installs roles or grants permissions.
use alien_permissions::{
    generators::aws_runtime::AwsRuntimePermissionsGenerator, get_permission_set, BindingTarget,
    PermissionContext,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let account = args.next().ok_or("Expected target account ID and region")?;
    let region = args.next().ok_or("Expected target region")?;
    let context = PermissionContext::new()
        .with_aws_account_id(&account)
        .with_aws_region(&region)
        .with_stack_prefix("metric-policy-review");
    let permission = get_permission_set("observe/metrics-read")
        .ok_or("Optional metric read permission is not registered")?;
    let policy = AwsRuntimePermissionsGenerator::new().generate_policy(
        permission,
        BindingTarget::Stack,
        &context,
    )?;
    println!("{}", serde_json::to_string(&policy)?);
    Ok(())
}
