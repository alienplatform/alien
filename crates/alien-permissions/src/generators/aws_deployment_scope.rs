use alien_core::{AwsBindingSpec, AwsPermissionEffect};
use indexmap::IndexMap;

type ConditionTemplate = IndexMap<String, IndexMap<String, String>>;

const DEPLOYMENT_TAG_KEY: &str = "aws:ResourceTag/${stackTag}";
const NAME_VARIABLES: [&str; 2] = ["${stackPrefix}", "${resourceName}"];

/// The condition template an AWS statement renders with.
///
/// A pattern that continues a deployment name with a wildcard, such as `${stackPrefix}-*`, also
/// matches the resources of a deployment whose prefix extends that name: `acme-*` matches
/// `acme-prod-db`. Every deployment tags its resources with its prefix, so an allow on such a
/// pattern also requires the resource's deployment tag, when it has one, to name this
/// deployment. Untagged resources, and requests AWS authorizes without resource tags, still
/// match by name alone.
pub(crate) fn condition_template(
    effect: &AwsPermissionEffect,
    binding: &AwsBindingSpec,
) -> Option<ConditionTemplate> {
    let declared = binding.condition.clone();
    let checks_deployment_tag = declared
        .iter()
        .flat_map(IndexMap::values)
        .any(|entries| entries.contains_key(DEPLOYMENT_TAG_KEY));
    let extends_a_name = binding
        .resources
        .iter()
        .any(|pattern| continues_a_name_with_wildcard(pattern));
    if !effect.is_allow() || checks_deployment_tag || !extends_a_name {
        return declared;
    }

    let mut condition = declared.unwrap_or_default();
    condition
        .entry("StringEqualsIfExists".to_string())
        .or_default()
        .insert(DEPLOYMENT_TAG_KEY.to_string(), "${stackPrefix}".to_string());
    Some(condition)
}

/// Whether a wildcard follows a name variable inside the same ARN segment. `${stackPrefix}-*`
/// and `${resourceName}-*-sa` do; `${resourceName}/*` and `log-group:${resourceName}:*` name
/// one resource and wildcard only what lies inside it.
fn continues_a_name_with_wildcard(pattern: &str) -> bool {
    NAME_VARIABLES.iter().any(|variable| {
        pattern.match_indices(variable).any(|(start, _)| {
            pattern[start + variable.len()..]
                .split(['/', ':'])
                .next()
                .is_some_and(|segment| segment.contains('*'))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards_after_a_name_are_recognized_within_its_segment() {
        for pattern in [
            "arn:aws:s3:::${stackPrefix}-*",
            "arn:aws:iam::1:role/${stackPrefix}-*-sa",
            "arn:aws:elasticloadbalancing:r:1:targetgroup/${stackPrefix}-*/*",
            "arn:aws:ssm:r:1:parameter/${resourceName}-*",
            "arn:aws:rds:r:1:db:${stackPrefix}-${resourceName}-*",
        ] {
            assert!(continues_a_name_with_wildcard(pattern), "{pattern}");
        }
        for pattern in [
            "arn:aws:s3:::${resourceName}/*",
            "arn:aws:logs:r:1:log-group:/aws/lambda/${resourceName}:*",
            "arn:aws:iam::1:role/${stackPrefix}-management",
            "arn:aws:ecr:*:1:repository/*",
            "*",
        ] {
            assert!(!continues_a_name_with_wildcard(pattern), "{pattern}");
        }
    }
}
