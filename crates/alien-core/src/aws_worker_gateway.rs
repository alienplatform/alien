//! Static configuration shared by setup-owned private worker gateways.
//!
//! The load balancer selects an allowed Host and prepends the API ID and endpoint
//! DNS name to the request path. The gateway restores the application path and
//! preserves Host while independently validating the upstream TLS identity.
//! Its security group must accept traffic only from that load balancer, and its
//! endpoint policy must allow only the associated API.

/// Envoy v1.39.4, pinned to its published multi-platform manifest.
pub const IMAGE: &str =
    "envoyproxy/envoy@sha256:74bd4eb5629708d3fadca1a5e6b85d208f338d7bb54e06b69565905c5294cc01";

/// Fixed bootstrap: no workload identity, IAM credentials or secrets are embedded.
pub const BOOTSTRAP: &str = include_str!("aws_worker_gateway/envoy.json");

/// Setup-owned task retirement capability. Publish an immutable Lambda version,
/// bind `ALIEN_STACK_PREFIX` at setup, and authorize only that exact version.
/// Its function name and role path must remain outside runtime worker grants.
pub const CLEANUP_HANDLER_SOURCE: &str = include_str!("aws_worker_gateway/cleanup.py");

/// This namespace cannot match ordinary `<resource-prefix>-*` worker grants.
pub const CLEANUP_FUNCTION_PREFIX: &str = "_alien-gateway-cleanup-";

/// Validate setup registration before exposing the capability to runtime.
pub fn validate_cleanup_function_arn(
    function_arn: &str,
    management_role_arn: &str,
    management_role_name: &str,
    region: &str,
) -> crate::Result<()> {
    let role: Vec<_> = management_role_arn.split(':').collect();
    let function: Vec<_> = function_arn.split(':').collect();
    let prefix = management_role_name.strip_suffix("-management");
    let valid = role.len() == 6
        && function.len() == 8
        && role[0] == "arn"
        && role[2] == "iam"
        && role[3].is_empty()
        && role[4].len() == 12
        && role[4].bytes().all(|byte| byte.is_ascii_digit())
        && role[5].starts_with("role/")
        && role[5].rsplit('/').next() == Some(management_role_name)
        && function[0] == "arn"
        && function[1] == role[1]
        && function[2] == "lambda"
        && !region.is_empty()
        && function[3] == region
        && function[4] == role[4]
        && function[5] == "function"
        && prefix.is_some_and(|prefix| {
            crate::is_valid_resource_prefix(prefix)
                && function[6] == format!("{CLEANUP_FUNCTION_PREFIX}{prefix}")
        })
        && !function[7].is_empty()
        && !function[7].starts_with('0')
        && function[7].bytes().all(|byte| byte.is_ascii_digit());
    if !valid {
        return Err(crate::ErrorData::GenericError {
            message: "Worker gateway cleanup must name a published setup function version in the management account, deployment prefix and region".to_string(),
        }
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod cleanup_tests {
    use super::validate_cleanup_function_arn;

    const ROLE: &str = "arn:aws:iam::123456789012:role/example-management";
    const FUNCTION: &str =
        "arn:aws:lambda:us-east-1:123456789012:function:_alien-gateway-cleanup-example:7";

    #[test]
    fn cleanup_import_accepts_only_the_exact_published_setup_boundary() {
        validate_cleanup_function_arn(FUNCTION, ROLE, "example-management", "us-east-1")
            .expect("published setup capability");
        for invalid in [
            FUNCTION.replace("123456789012", "999999999999"),
            FUNCTION.replace("us-east-1", "eu-west-1"),
            FUNCTION.replace("arn:aws:", "arn:aws-cn:"),
            FUNCTION.replace("cleanup-example", "cleanup-example-extra"),
            FUNCTION.replace("_alien-gateway-cleanup-example", "example-worker"),
            FUNCTION.replace(":7", ":$LATEST"),
            FUNCTION.replace(":7", ":current"),
            FUNCTION.replace(":7", ""),
            FUNCTION.replace(":7", ":0"),
        ] {
            validate_cleanup_function_arn(&invalid, ROLE, "example-management", "us-east-1")
                .expect_err("foreign or mutable capability must be refused");
        }
    }
}

/// Application traffic accepted only from the associated load balancer.
pub const HTTP_PORT: u16 = 8080;

/// Separate health listener so every application path remains available.
pub const HEALTH_PORT: u16 = 8081;

/// Gateway-authored application target, before REST API path/query decoding.
/// The gateway overwrites caller values; the private REST adapter consumes it.
pub const ORIGINAL_TARGET_HEADER: &str = "x-alien-original-target";
