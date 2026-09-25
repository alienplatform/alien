//! The IAM role an AWS sandbox image build runs as, as concrete policy documents.
//!
//! The CloudFormation and Terraform emitters write the same role as template expressions. This is
//! the resolved form for a caller that creates the role through the IAM API, and the generator
//! parity tests fail if the three ever disagree.
//!
//! Field names are PascalCase because that is the IAM policy wire format.

use crate::{
    parse_bundle_uri, parse_ecr_image_repository, stable_bundle_key_prefix, BundleUri, ErrorData,
    Result,
};
use alien_error::AlienError;
use serde::{Deserialize, Serialize};

/// The name of the build role's inline policy.
pub const SANDBOX_BUILD_POLICY_NAME: &str = "sandbox-image-build";

const IAM_POLICY_VERSION: &str = "2012-10-17";

/// The one derivation of the build role's name: the step that creates the role and the controller
/// that passes it must agree. Never clamped, because `SandboxBuildRoleNameCheck` refuses any id
/// that could reach IAM's 64-character ceiling and `iam:PassRole` is scoped to this exact name.
pub fn sandbox_build_role_name(resource_prefix: &str, sandbox_id: &str) -> String {
    format!("{resource_prefix}-{sandbox_id}-build")
}

/// The ARN of [`sandbox_build_role_name`] at the root path, where the role is created.
pub fn sandbox_build_role_arn(
    partition: &str,
    account_id: &str,
    resource_prefix: &str,
    sandbox_id: &str,
) -> String {
    format!(
        "arn:{partition}:iam::{account_id}:role/{}",
        sandbox_build_role_name(resource_prefix, sandbox_id)
    )
}

/// Whether a statement grants or refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IamEffect {
    Allow,
    Deny,
}

/// The build role's inline permission policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SandboxBuildPolicy {
    pub version: String,
    pub statement: Vec<SandboxBuildStatement>,
}

/// One statement of the build role's permission policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SandboxBuildStatement {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sid: Option<String>,
    pub effect: IamEffect,
    pub action: Vec<String>,
    pub resource: String,
}

/// Who may assume the build role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SandboxBuildTrustPolicy {
    pub version: String,
    pub statement: Vec<SandboxBuildTrustStatement>,
}

/// A service principal allowed to assume the role from one source account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SandboxBuildTrustStatement {
    pub effect: IamEffect,
    pub principal: ServicePrincipal,
    pub action: String,
    pub condition: SourceAccountCondition,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ServicePrincipal {
    pub service: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceAccountCondition {
    #[serde(rename = "StringEquals")]
    pub string_equals: SourceAccount,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceAccount {
    #[serde(rename = "aws:SourceAccount")]
    pub source_account: String,
}

/// The values one sandbox's build role is resolved against.
#[derive(Debug, Clone, bon::Builder)]
pub struct SandboxBuildRole<'a> {
    sandbox_id: &'a str,
    partition: &'a str,
    account_id: &'a str,
    region: &'a str,
    /// The sandbox's `code.image` as declared; a `{region}` token resolves to `region`.
    bundle_uri: &'a str,
    /// A Live sandbox, whose image the runtime rebuilds from each new bundle.
    runtime_built: bool,
    /// The sandbox's `privateBaseImage`; a `{region}` token in its host resolves to `region`.
    private_base_image: Option<&'a str>,
}

impl SandboxBuildRole<'_> {
    /// Read the bundle, and pull the declared private base image. No logs grant: every path
    /// builds the image with logging disabled.
    ///
    /// A Frozen image is built once from the named object; a runtime rebuild reads a new key under
    /// the same stable prefix, so a Live role reads the prefix and is refused when there is none.
    pub fn policy(&self) -> Result<SandboxBuildPolicy> {
        let path = self.bundle_path()?;
        let (bucket, key) = path
            .split_once('/')
            .unwrap_or_else(|| unreachable!("parse_bundle_uri refuses a URI with no object key"));
        let partition = self.partition;

        let bundle_grant = if self.runtime_built {
            let prefix = stable_bundle_key_prefix(key).ok_or_else(|| {
                self.refuse(format!(
                    "code.image key '{key}' has no stable prefix above its version segment, so \
                     a runtime rebuild's new key cannot be granted; publish the bundle as \
                     s3://bucket/sandbox-bundle/<version>/bundle.zip"
                ))
            })?;
            allow(
                "ReadSandboxBundlePrefix",
                &["s3:GetObject"],
                format!("arn:{partition}:s3:::{bucket}/{prefix}/*"),
            )
        } else {
            allow(
                "ReadSandboxBundle",
                &["s3:GetObject"],
                format!("arn:{partition}:s3:::{path}"),
            )
        };

        let mut statement = vec![bundle_grant];
        // No declared base means a public one, pulled anonymously, so no ECR grant at all.
        if let Some(image) = self.private_base_image {
            let repository =
                parse_ecr_image_repository(image).map_err(|reason| self.refuse(reason))?;
            // GetAuthorizationToken has no resource type, so it can only be granted on `*`.
            statement.push(allow(
                "AuthorizeSandboxBaseImagePull",
                &["ecr:GetAuthorizationToken"],
                "*".to_string(),
            ));
            statement.push(allow(
                "PullSandboxBaseImage",
                &["ecr:BatchGetImage", "ecr:GetDownloadUrlForLayer"],
                repository.arn(partition, self.region),
            ));
            // A private base comes from another account's registry; one in this account would let the
            // build read this account's own repositories. The templates cannot know the account when
            // they render, so a Deny refuses it on every path.
            statement.push(SandboxBuildStatement {
                sid: Some("DenySameAccountImagePull".to_string()),
                effect: IamEffect::Deny,
                action: actions(&["ecr:BatchGetImage", "ecr:GetDownloadUrlForLayer"]),
                resource: format!("arn:{partition}:ecr:*:{}:repository/*", self.account_id),
            });
        }

        Ok(SandboxBuildPolicy {
            version: IAM_POLICY_VERSION.to_string(),
            statement,
        })
    }

    /// Lambda, and only on behalf of this account: AWS's confused-deputy guidance for the role.
    pub fn trust_policy(&self) -> SandboxBuildTrustPolicy {
        SandboxBuildTrustPolicy {
            version: IAM_POLICY_VERSION.to_string(),
            statement: vec![SandboxBuildTrustStatement {
                effect: IamEffect::Allow,
                principal: ServicePrincipal {
                    service: "lambda.amazonaws.com".to_string(),
                },
                action: "sts:AssumeRole".to_string(),
                condition: SourceAccountCondition {
                    string_equals: SourceAccount {
                        source_account: self.account_id.to_string(),
                    },
                },
            }],
        }
    }

    /// The bucket-and-key path the bundle URI addresses, with the region token resolved.
    fn bundle_path(&self) -> Result<String> {
        match parse_bundle_uri(self.bundle_uri).map_err(|reason| self.refuse(reason))? {
            BundleUri::Literal(uri) => Ok(uri.trim_start_matches("s3://").to_string()),
            BundleUri::Regional { before, after } => Ok(format!(
                "{}{}{after}",
                before.trim_start_matches("s3://"),
                self.region
            )),
        }
    }

    fn refuse(&self, reason: String) -> AlienError<ErrorData> {
        AlienError::new(ErrorData::OperationNotSupported {
            operation: format!("build role policy for sandbox '{}'", self.sandbox_id),
            reason,
        })
    }
}

fn allow(sid: &str, action: &[&str], resource: String) -> SandboxBuildStatement {
    SandboxBuildStatement {
        sid: Some(sid.to_string()),
        effect: IamEffect::Allow,
        action: actions(action),
        resource,
    }
}

fn actions(action: &[&str]) -> Vec<String> {
    action.iter().map(|a| a.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const PARTITION: &str = "aws-us-gov";
    const ACCOUNT: &str = "210987654321";
    const REGION: &str = "us-gov-west-1";

    const BASE_IMAGE: &str = "123456789012.dkr.ecr.{region}.amazonaws.com/acme/agents-base:1.4";

    fn role(bundle_uri: &str, runtime_built: bool) -> SandboxBuildRole<'_> {
        role_with_base(bundle_uri, runtime_built, None)
    }

    fn role_with_base<'a>(
        bundle_uri: &'a str,
        runtime_built: bool,
        private_base_image: Option<&'a str>,
    ) -> SandboxBuildRole<'a> {
        SandboxBuildRole::builder()
            .sandbox_id("agents")
            .partition(PARTITION)
            .account_id(ACCOUNT)
            .region(REGION)
            .bundle_uri(bundle_uri)
            .runtime_built(runtime_built)
            .maybe_private_base_image(private_base_image)
            .build()
    }

    fn policy_json(bundle_uri: &str, runtime_built: bool) -> serde_json::Value {
        policy_json_with_base(bundle_uri, runtime_built, None)
    }

    fn policy_json_with_base(
        bundle_uri: &str,
        runtime_built: bool,
        private_base_image: Option<&str>,
    ) -> serde_json::Value {
        serde_json::to_value(
            role_with_base(bundle_uri, runtime_built, private_base_image)
                .policy()
                .expect("policy builds"),
        )
        .expect("serializes")
    }

    #[test]
    fn a_frozen_role_reads_one_object_and_pulls_nothing() {
        assert_eq!(
            policy_json("s3://acme-artifacts/agents/bundle.zip", false),
            json!({
                "Version": "2012-10-17",
                "Statement": [
                    {
                        "Sid": "ReadSandboxBundle",
                        "Effect": "Allow",
                        "Action": ["s3:GetObject"],
                        "Resource": "arn:aws-us-gov:s3:::acme-artifacts/agents/bundle.zip"
                    }
                ]
            })
        );
    }

    #[test]
    fn a_live_role_with_no_private_base_pulls_nothing() {
        assert_eq!(
            policy_json(
                "s3://acme-artifacts/sandbox-bundle/f00dcafe/bundle.zip",
                true
            ),
            json!({
                "Version": "2012-10-17",
                "Statement": [
                    {
                        "Sid": "ReadSandboxBundlePrefix",
                        "Effect": "Allow",
                        "Action": ["s3:GetObject"],
                        "Resource": "arn:aws-us-gov:s3:::acme-artifacts/sandbox-bundle/*"
                    }
                ]
            })
        );
    }

    #[test]
    fn a_private_base_is_pulled_from_its_one_repository_and_never_this_account() {
        assert_eq!(
            policy_json_with_base(
                "s3://acme-artifacts/sandbox-bundle/f00dcafe/bundle.zip",
                true,
                Some(BASE_IMAGE)
            ),
            json!({
                "Version": "2012-10-17",
                "Statement": [
                    {
                        "Sid": "ReadSandboxBundlePrefix",
                        "Effect": "Allow",
                        "Action": ["s3:GetObject"],
                        "Resource": "arn:aws-us-gov:s3:::acme-artifacts/sandbox-bundle/*"
                    },
                    {
                        "Sid": "AuthorizeSandboxBaseImagePull",
                        "Effect": "Allow",
                        "Action": ["ecr:GetAuthorizationToken"],
                        "Resource": "*"
                    },
                    {
                        "Sid": "PullSandboxBaseImage",
                        "Effect": "Allow",
                        "Action": ["ecr:BatchGetImage", "ecr:GetDownloadUrlForLayer"],
                        "Resource": "arn:aws-us-gov:ecr:us-gov-west-1:123456789012:repository/acme/agents-base"
                    },
                    {
                        "Sid": "DenySameAccountImagePull",
                        "Effect": "Deny",
                        "Action": ["ecr:BatchGetImage", "ecr:GetDownloadUrlForLayer"],
                        "Resource": "arn:aws-us-gov:ecr:*:210987654321:repository/*"
                    }
                ]
            })
        );
    }

    #[test]
    fn a_private_base_outside_ecr_is_refused() {
        let error = role_with_base(
            "s3://acme-artifacts/sandbox-bundle/f00dcafe/bundle.zip",
            true,
            Some("docker.io/library/alpine:3.20"),
        )
        .policy()
        .expect_err("only an ECR repository can be granted");
        assert_eq!(error.code, "OPERATION_NOT_SUPPORTED");
    }

    #[test]
    fn the_region_token_resolves_into_both_grants() {
        let frozen = role("s3://acme-{region}/agents/bundle.zip", false)
            .policy()
            .expect("policy builds");
        let live = role(
            "s3://acme-{region}/sandbox-bundle/f00dcafe/bundle.zip",
            true,
        )
        .policy()
        .expect("policy builds");

        assert_eq!(
            frozen.statement[0].resource,
            "arn:aws-us-gov:s3:::acme-us-gov-west-1/agents/bundle.zip"
        );
        assert_eq!(
            live.statement[0].resource,
            "arn:aws-us-gov:s3:::acme-us-gov-west-1/sandbox-bundle/*"
        );
    }

    #[test]
    fn a_live_role_is_refused_a_key_with_no_stable_prefix() {
        let error = role("s3://acme-artifacts/bundle.zip", true)
            .policy()
            .expect_err("a flat key has no prefix a rebuild stays under");
        assert_eq!(error.code, "OPERATION_NOT_SUPPORTED");

        role("s3://acme-artifacts/bundle.zip", false)
            .policy()
            .expect("a Frozen role reads the one object and needs no prefix");
    }

    #[test]
    fn a_wildcard_in_the_bundle_path_is_refused() {
        for uri in [
            "s3://acme-artifacts/agents/*.zip",
            "s3://acme-artifacts/sandbox-bundle/?/bundle.zip",
        ] {
            let error = role(uri, true)
                .policy()
                .expect_err("a wildcard would widen the grant past the bundle");
            assert_eq!(error.code, "OPERATION_NOT_SUPPORTED", "{uri}");
        }
    }

    #[test]
    fn the_trust_policy_admits_lambda_for_this_account_only() {
        assert_eq!(
            serde_json::to_value(
                role("s3://acme-artifacts/agents/bundle.zip", false).trust_policy()
            )
            .expect("serializes"),
            json!({
                "Version": "2012-10-17",
                "Statement": [{
                    "Effect": "Allow",
                    "Principal": { "Service": "lambda.amazonaws.com" },
                    "Action": "sts:AssumeRole",
                    "Condition": { "StringEquals": { "aws:SourceAccount": "210987654321" } }
                }]
            })
        );
    }
}
