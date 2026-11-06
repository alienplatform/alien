//! Render the setup policy with concrete fixture values for IAM validation.
//! This contains no credentials and makes no cloud requests. Feed stdout to
//! Access Analyzer ValidatePolicy and IAM SimulateCustomPolicy; include allowed
//! and foreign names, absent/mismatched Export, region, DNS method, ancestor
//! SSM paths, import/export, and sibling-certificate tagging contexts.
use alien_permissions::generators::signing_limits::{
    aws_management_limits_with_custody, aws_private_managed_certificate_policy,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut policy =
        aws_management_limits_with_custody("fixture", "111122223333", "us-east-1", true);
    let certificates = aws_private_managed_certificate_policy(
        "111122223333",
        "us-east-1",
        serde_json::json!(["app.example.com", "api.example.com"]),
    );
    policy["Statement"]
        .as_array_mut()
        .expect("generated policy")
        .extend(
            certificates["Statement"]
                .as_array()
                .expect("generated certificate policy")
                .iter()
                .cloned(),
        );
    println!("{}", serde_json::to_string_pretty(&policy)?);
    Ok(())
}
