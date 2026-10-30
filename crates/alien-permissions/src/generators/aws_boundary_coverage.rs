//! Conservative coverage checks for setup-generated permissions boundaries.

use alien_error::AlienError;
use serde_json::Value;

use super::aws_boundary::action_pattern_matches;
use crate::{ErrorData, Result};

/// Whether the installed boundary permits everything the required boundary permits.
///
/// This compares generated identity policies, not arbitrary IAM policy equivalence. Conditions
/// on the installed grant must also occur in the required grant. Each installed Deny must
/// occur in the required document or provably match none of its Allows. Unsupported
/// comparisons return false, requiring setup again;
/// malformed documents return an error. Neither case may be treated as permission to proceed.
/// Documents must be decoded JSON with template parameters already resolved.
pub fn aws_permissions_boundary_covers(installed: &Value, required: &Value) -> Result<bool> {
    let installed = statements(installed)?;
    let required = statements(required)?;
    let required_allows: Vec<_> = required.iter().filter(|s| s["Effect"] == "Allow").collect();
    for deny in installed.iter().filter(|s| s["Effect"] == "Deny") {
        if !required.iter().any(|s| same_statement(deny, s))
            && required_allows
                .iter()
                .any(|allow| deny_may_match(deny, allow))
        {
            return Ok(false);
        }
    }
    for wanted in required.iter().filter(|s| s["Effect"] == "Allow") {
        let action_key = if wanted.get("Action").is_some() {
            "Action"
        } else {
            "NotAction"
        };
        let resource_key = if wanted.get("Resource").is_some() {
            "Resource"
        } else {
            "NotResource"
        };
        for action in strings(&wanted[action_key])? {
            for resource in strings(&wanted[resource_key])? {
                let covered = installed
                    .iter()
                    .filter(|s| s["Effect"] == "Allow")
                    .any(|grant| {
                        let condition_covered =
                            grant.get("Condition").is_none_or(|conditions| {
                                conditions.as_object().into_iter().flatten().all(
                                    |(operator, entries)| {
                                        entries.as_object().into_iter().flatten().all(
                                            |(key, value)| {
                                                wanted
                                                    .get("Condition")
                                                    .and_then(|conditions| conditions.get(operator))
                                                    .and_then(|entries| entries.get(key))
                                                    == Some(value)
                                            },
                                        )
                                    },
                                )
                            });
                        condition_covered
                            && action_covered(grant, wanted, action_key, action)
                            && resource_covered(grant, wanted, resource_key, resource)
                    });
                if !covered {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

fn invalid(message: &str) -> alien_error::AlienError<ErrorData> {
    AlienError::new(ErrorData::InvalidPermissionSet {
        message: format!("Cannot compare AWS permissions boundaries: {message}"),
    })
}

fn statements(document: &Value) -> Result<Vec<&Value>> {
    let object = document
        .as_object()
        .ok_or_else(|| invalid("policy must be an object"))?;
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "Version" | "Id" | "Statement"))
        || document["Version"] != "2012-10-17"
    {
        return Err(invalid("unsupported policy fields or version"));
    }
    let statements = match &document["Statement"] {
        Value::Array(statements) => statements.iter().collect::<Vec<_>>(),
        statement @ Value::Object(_) => vec![statement],
        _ => return Err(invalid("Statement must be an object or array")),
    };
    for statement in &statements {
        let object = statement
            .as_object()
            .ok_or_else(|| invalid("statement must be an object"))?;
        if object.keys().any(|key| {
            !matches!(
                key.as_str(),
                "Sid"
                    | "Effect"
                    | "Action"
                    | "NotAction"
                    | "Resource"
                    | "NotResource"
                    | "Condition"
            )
        }) || !matches!(statement["Effect"].as_str(), Some("Allow" | "Deny"))
        {
            return Err(invalid("unsupported statement fields or effect"));
        }
        for (positive, negative) in [("Action", "NotAction"), ("Resource", "NotResource")] {
            match (statement.get(positive), statement.get(negative)) {
                (Some(value), None) | (None, Some(value)) => {
                    for selector in strings(value)? {
                        validate_variables(selector, positive == "Resource")?;
                    }
                }
                _ => {
                    return Err(invalid(
                        "statement requires exactly one action and resource selector",
                    ))
                }
            }
        }
        if let Some(condition) = statement.get("Condition") {
            validate_condition(condition)?;
        }
    }
    Ok(statements)
}

// Validate the decoded structure before comparing grants. In particular, an unused
// malformed statement must not be hidden by another Allow that covers every request.
fn validate_condition(condition: &Value) -> Result<()> {
    let operators = condition
        .as_object()
        .filter(|operators| !operators.is_empty())
        .ok_or_else(|| invalid("Condition must be a nonempty object"))?;
    for (operator, entries) in operators {
        if operator.is_empty() {
            return Err(invalid("condition operator must not be empty"));
        }
        validate_variables(operator, false)?;
        let entries = entries
            .as_object()
            .filter(|entries| !entries.is_empty())
            .ok_or_else(|| {
                invalid("condition operator must contain a nonempty key/value object")
            })?;
        for (key, value) in entries {
            if key.is_empty() {
                return Err(invalid("condition key must not be empty"));
            }
            validate_variables(key, false)?;
            match value {
                Value::Array(values) if !values.is_empty() => {
                    for value in values {
                        validate_condition_value(value)?;
                    }
                }
                value => validate_condition_value(value)?,
            }
        }
    }
    Ok(())
}

fn validate_condition_value(value: &Value) -> Result<()> {
    match value {
        Value::String(value) => validate_variables(value, true),
        Value::Bool(_) | Value::Number(_) => Ok(()),
        _ => Err(invalid(
            "condition values must be strings, numbers, booleans, or nonempty arrays of them",
        )),
    }
}

fn validate_variables(mut value: &str, allow_iam_variables: bool) -> Result<()> {
    while let Some((_, rest)) = value.split_once("${") {
        let (variable, remaining) = rest
            .split_once('}')
            .ok_or_else(|| invalid("policy contains an unterminated variable"))?;
        // IAM request variables remain symbolic after setup. Template inputs such
        // as ${local.resource_prefix}, ${BoundaryPolicy}, and ${AWS::AccountId}
        // must already have been resolved. IAM also supports literal escapes and
        // defaults, e.g. ${aws:PrincipalTag/team, 'unassigned'}.
        if !allow_iam_variables || !crate::variables::is_iam_policy_variable(variable) {
            return Err(invalid("policy contains an unresolved setup variable"));
        }
        value = remaining;
    }
    Ok(())
}

fn strings(value: &Value) -> Result<Vec<&str>> {
    let values = match value {
        Value::String(value) => vec![value.as_str()],
        Value::Array(values) => values
            .iter()
            .map(|v| {
                v.as_str()
                    .ok_or_else(|| invalid("selectors must contain strings"))
            })
            .collect::<Result<Vec<_>>>()?,
        _ => return Err(invalid("selector must be a string or array")),
    };
    if values.is_empty() || values.iter().any(|value| value.is_empty()) {
        return Err(invalid("selector must not be empty"));
    }
    Ok(values)
}

/// Conservative: true unless the Deny's actions or condition provably exclude every
/// request the Allow grants. Only an all-resource Deny with an `Action` list, optionally
/// limited by `BoolIfExists {key: "false"}`, can be shown not to match.
fn deny_may_match(deny: &Value, allow: &Value) -> bool {
    let (Ok(denied), true) = (
        strings(&deny["Action"]),
        deny.get("Resource") == Some(&Value::from("*")),
    ) else {
        return true;
    };
    if let Some(condition) = deny.get("Condition") {
        let excluded = condition.as_object().is_some_and(|operators| {
            operators.len() == 1
                && operators
                    .get("BoolIfExists")
                    .and_then(Value::as_object)
                    .is_some_and(|keys| {
                        keys.len() == 1
                            && keys.iter().all(|(key, value)| {
                                value == "false" && allow["Condition"]["Bool"][key] == "true"
                            })
                    })
        });
        if excluded {
            return false;
        }
        if !condition
            .as_object()
            .is_some_and(|operators| operators.keys().all(|op| op == "BoolIfExists"))
        {
            return true;
        }
    }
    denied.into_iter().any(|action| {
        if let Some(allowed) = allow.get("Action") {
            strings(allowed).map_or(true, |patterns| {
                patterns.iter().any(|pattern| {
                    action_pattern_matches(pattern, action)
                        || action_pattern_matches(action, pattern)
                })
            })
        } else {
            strings(&allow["NotAction"]).map_or(true, |excluded| {
                !excluded
                    .iter()
                    .any(|pattern| action_pattern_matches(pattern, action))
            })
        }
    })
}

fn same_statement(left: &Value, right: &Value) -> bool {
    [
        "Effect",
        "Action",
        "NotAction",
        "Resource",
        "NotResource",
        "Condition",
    ]
    .iter()
    .all(|key| left.get(key) == right.get(key))
}

fn action_covered(grant: &Value, wanted: &Value, key: &str, action: &str) -> bool {
    // Complements cannot be split into separate grants: NotAction [a, b] is narrower than
    // either NotAction a or NotAction b. Compare the entire exclusion list.
    if key == "NotAction" {
        return grant.get("NotAction") == wanted.get("NotAction")
            || grant
                .get("Action")
                .is_some_and(|value| has_pattern(value, "*", true));
    }
    if let Some(actions) = grant.get("Action") {
        return has_pattern(actions, action, true);
    }
    let excludes_only_iam = strings(&grant["NotAction"])
        .is_ok_and(|actions| actions.len() == 1 && actions[0].eq_ignore_ascii_case("iam:*"));
    excludes_only_iam
        && action.split_once(':').is_some_and(|(service, _)| {
            !service.eq_ignore_ascii_case("iam")
                && !service.is_empty()
                && !service.contains(['*', '?', '$'])
        })
}

fn resource_covered(grant: &Value, wanted: &Value, key: &str, resource: &str) -> bool {
    if key == "NotResource" {
        return grant.get("NotResource") == wanted.get("NotResource")
            || grant
                .get("Resource")
                .is_some_and(|value| has_pattern(value, "*", false));
    }
    grant
        .get("Resource")
        .is_some_and(|value| has_pattern(value, resource, false))
}

fn has_pattern(value: &Value, wanted: &str, ignore_case: bool) -> bool {
    strings(value).is_ok_and(|patterns| {
        patterns.into_iter().any(|pattern| {
            if ignore_case {
                pattern_covers(&pattern.to_ascii_lowercase(), &wanted.to_ascii_lowercase())
            } else {
                pattern_covers(pattern, wanted)
            }
        })
    })
}

fn pattern_covers(installed: &str, required: &str) -> bool {
    if installed == required || installed == "*" {
        return true;
    }
    // IAM variables are evaluated per request, not literal text to be wildcard-matched.
    if installed.contains("${") || required.contains("${") {
        return false;
    }
    if let Some(prefix) = installed.strip_suffix('*') {
        if !prefix.contains(['*', '?']) && required.starts_with(prefix) {
            return true;
        }
    }
    // Removing an identical glob suffix is a sufficient (not necessary) subset proof.
    // This handles catalog ARNs such as prefix-*:* covering prefix-resource:*.
    let common_suffix = installed
        .chars()
        .rev()
        .zip(required.chars().rev())
        .take_while(|(left, right)| left == right)
        .map(|(character, _)| character.len_utf8())
        .sum::<usize>();
    let installed = &installed[..installed.len() - common_suffix];
    let required = &required[..required.len() - common_suffix];
    if let Some(prefix) = installed.strip_suffix('*') {
        if !prefix.contains(['*', '?']) && required.starts_with(prefix) {
            return true;
        }
    }
    // Matching a requested wildcard as literal text is not a subset proof (e.g. '?' vs '*').
    if required.contains(['*', '?']) {
        return false;
    }
    let expression = regex::escape(installed)
        .replace(r"\*", ".*")
        .replace(r"\?", ".");
    regex::Regex::new(&format!("\\A{expression}\\z")).is_ok_and(|re| re.is_match(required))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{generators::aws_permissions_boundary, PermissionContext};

    fn policy(statements: Value) -> Value {
        json!({"Version": "2012-10-17", "Statement": statements})
    }

    fn allow(action: &str, resource: &str) -> Value {
        json!({"Effect": "Allow", "Action": action, "Resource": resource})
    }

    #[test]
    fn generated_boundary_detects_catalog_and_custom_grant_growth() {
        let context = PermissionContext::new()
            .with_stack_prefix("sample")
            .with_aws_account_id("123456789012")
            .with_aws_region("us-east-1")
            .with_managing_account_id("210987654321");
        let installed = aws_permissions_boundary(&context, &[]).unwrap();
        assert!(aws_permissions_boundary_covers(&installed, &installed).unwrap());
        let mut required = installed.clone();
        required["Statement"]
            .as_array_mut()
            .unwrap()
            .push(allow("s3:GetObject", "arn:aws:s3:::external-approved/*"));
        assert!(!aws_permissions_boundary_covers(&installed, &required).unwrap());
        assert!(aws_permissions_boundary_covers(&required, &installed).unwrap());
        let mut old = installed.clone();
        old["Statement"][0]["Resource"]
            .as_array_mut()
            .unwrap()
            .pop();
        assert!(!aws_permissions_boundary_covers(&old, &installed).unwrap());
    }

    #[test]
    fn coverage_survives_order_chunking_sids_and_scalar_selectors() {
        let installed = policy(json!([
            allow("s3:Get*", "arn:aws:s3:::sample-*"),
            allow("s3:PutObject", "arn:aws:s3:::sample-*"),
        ]));
        let required = policy(json!([{
            "Sid": "Changed", "Effect": "Allow",
            "Action": ["s3:putobject", "S3:GetObject", "s3:GetObjectVersion"],
            "Resource": ["arn:aws:s3:::sample-one/*", "arn:aws:s3:::sample-two/*"]
        }]));
        assert!(aws_permissions_boundary_covers(&installed, &required).unwrap());
        let cross_product = policy(json!([
            allow("s3:GetObject", "arn:aws:s3:::sample-one/*"),
            allow("s3:PutObject", "arn:aws:s3:::sample-two/*"),
        ]));
        assert!(!aws_permissions_boundary_covers(&cross_product, &required).unwrap());
    }

    #[test]
    fn wildcard_comparisons_never_treat_requested_patterns_as_literals() {
        for (installed, required, covered) in [
            ("prefix?", "prefix*", false),
            ("prefix*", "prefix?", true),
            ("prefix*", "pre*", false),
            ("a*b", "ax*b", false), // conservative, not general glob inclusion
            ("a*b", "axxb", true),
            ("a?b", "ab", false),
            ("a?b", "axb", true),
            ("a*b", "axxb\n", false),
            ("arn:aws:s3:::Sample-*", "arn:aws:s3:::sample-*", false),
            ("prefix*", "prefix${aws:username}", false),
            ("*", "prefix${aws:username}", true),
        ] {
            assert_eq!(
                pattern_covers(installed, required),
                covered,
                "{installed} vs {required}"
            );
        }
    }

    #[test]
    fn accepted_glob_subsets_cover_all_short_example_strings() {
        fn words(alphabet: &[char], max_length: usize) -> Vec<String> {
            let mut all = vec![String::new()];
            let mut layer = all.clone();
            for _ in 0..max_length {
                layer = layer
                    .iter()
                    .flat_map(|word| alphabet.iter().map(move |ch| format!("{word}{ch}")))
                    .collect();
                all.extend(layer.iter().cloned());
            }
            all
        }
        let examples = words(&['a', 'b'], 5);
        let patterns = words(&['a', 'b', '*', '?'], 3);
        let matches: Vec<Vec<bool>> = patterns
            .iter()
            .map(|pattern| {
                let expression = regex::escape(pattern)
                    .replace(r"\*", ".*")
                    .replace(r"\?", ".");
                let regex = regex::Regex::new(&format!("\\A{expression}\\z")).unwrap();
                examples
                    .iter()
                    .map(|example| regex.is_match(example))
                    .collect()
            })
            .collect();
        for (i, installed) in patterns.iter().enumerate() {
            for (r, required) in patterns.iter().enumerate() {
                if pattern_covers(installed, required) {
                    for (e, example) in examples.iter().enumerate() {
                        assert!(
                            !matches[r][e] || matches[i][e],
                            "{installed} does not cover {required}: counterexample {example:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn non_iam_ceiling_excludes_iam_and_wildcard_services() {
        let installed = policy(json!({
            "Effect": "Allow", "NotAction": "iam:*", "Resource": "*"
        }));
        for action in ["s3:*", "EC2:Describe*", "bedrock:InvokeModel"] {
            assert!(
                aws_permissions_boundary_covers(&installed, &policy(allow(action, "*"))).unwrap()
            );
        }
        for action in ["iam:GetRole", "IAM:*", "*", "*:GetRole", "i?m:*"] {
            assert!(
                !aws_permissions_boundary_covers(&installed, &policy(allow(action, "*"))).unwrap()
            );
        }
        let required = policy(json!({
            "Effect": "Allow", "NotAction": ["iam:*", "s3:*"], "Resource": "*"
        }));
        assert!(!aws_permissions_boundary_covers(&required, &installed).unwrap());
        assert!(aws_permissions_boundary_covers(&required, &required).unwrap());
    }

    #[test]
    fn conditions_cannot_be_strengthened_and_installed_denies_cannot_be_ignored() {
        let unconditional = policy(allow("s3:GetObject", "*"));
        let mut conditional = unconditional.clone();
        conditional["Statement"]["Condition"] =
            json!({"StringEquals": {"aws:RequestedRegion": "us-east-1"}});
        assert!(aws_permissions_boundary_covers(&unconditional, &conditional).unwrap());
        assert!(!aws_permissions_boundary_covers(&conditional, &unconditional).unwrap());
        assert!(aws_permissions_boundary_covers(&conditional, &conditional).unwrap());
        let mut changed = conditional.clone();
        changed["Statement"]["Condition"]["StringEquals"]["aws:RequestedRegion"] =
            json!("us-west-2");
        assert!(!aws_permissions_boundary_covers(&conditional, &changed).unwrap());

        let mut narrower = conditional.clone();
        narrower["Statement"]["Condition"]["StringEquals"]["aws:ResourceTag/deployment"] =
            json!("our-stack");
        narrower["Statement"]["Condition"]["Bool"] = json!({"aws:SecureTransport": "true"});
        assert!(aws_permissions_boundary_covers(&conditional, &narrower).unwrap());
        assert!(!aws_permissions_boundary_covers(&narrower, &conditional).unwrap());
        let mut optional = conditional.clone();
        optional["Statement"]["Condition"] =
            json!({"StringEqualsIfExists": {"aws:RequestedRegion": "us-east-1"}});
        assert!(!aws_permissions_boundary_covers(&conditional, &optional).unwrap());

        let denied = policy(json!([
            allow("s3:GetObject", "*"),
            {"Effect": "Deny", "Action": "s3:GetObject", "Resource": "arn:aws:s3:::restricted/*"}
        ]));
        assert!(!aws_permissions_boundary_covers(&denied, &unconditional).unwrap());
        assert!(aws_permissions_boundary_covers(&unconditional, &denied).unwrap());
        assert!(aws_permissions_boundary_covers(&denied, &denied).unwrap());
    }

    #[test]
    fn not_resource_exclusions_are_compared_as_a_whole() {
        let installed = policy(json!({"Effect": "Allow", "Action": "s3:GetObject",
            "NotResource": ["arn:aws:s3:::one/*", "arn:aws:s3:::two/*"]}));
        assert!(aws_permissions_boundary_covers(&installed, &installed).unwrap());
        let mut required = installed.clone();
        required["Statement"]["NotResource"] = json!(["arn:aws:s3:::one/*"]);
        assert!(!aws_permissions_boundary_covers(&installed, &required).unwrap());
        assert!(aws_permissions_boundary_covers(&policy(allow("s3:*", "*")), &required).unwrap());
    }

    #[test]
    fn unresolved_setup_variables_fail_even_when_identical_or_covered_by_a_wildcard() {
        let broad = policy(allow("*", "*"));
        for token in [
            "${stackPrefix}",
            "${resourceName}",
            "${awsAccountId}",
            "${AWS::StackName}",
            "${AWS::AccountId}",
            "${BoundaryPolicy}",
            "${local.resource_prefix}",
            "${var.region}",
            "${data.aws_caller_identity.current.account_id}",
            "${aws_iam_policy.boundary.arn}",
            "${var.enabled?local.first:local.second}",
            "${unterminated",
        ] {
            let resource = policy(allow("s3:GetObject", &format!("arn:aws:s3:::{token}/*")));
            let mut condition = policy(allow("s3:GetObject", "*"));
            condition["Statement"]["Condition"] =
                json!({"StringEquals": {"aws:RequestedRegion": token}});
            for unresolved in [resource, condition] {
                for (installed, required) in [
                    (&unresolved, &unresolved),
                    (&broad, &unresolved),
                    (&unresolved, &broad),
                ] {
                    assert_eq!(
                        aws_permissions_boundary_covers(installed, required)
                            .unwrap_err()
                            .code,
                        "INVALID_PERMISSION_SET",
                        "{token}"
                    );
                }
            }
        }
        let action = policy(allow("${service}:*", "*"));
        assert!(aws_permissions_boundary_covers(&broad, &action).is_err());
    }

    #[test]
    fn iam_request_variables_and_scalar_condition_types_remain_usable() {
        for token in [
            "${aws:username}",
            "${aws:PrincipalTag/team}",
            "${aws:PrincipalTag/team name}",
            "${aws:PrincipalTag/team, 'unassigned'}",
            "${*}",
            "${?}",
            "${$}",
        ] {
            let variable = policy(allow(
                "s3:GetObject",
                &format!("arn:aws:s3:::bucket/{token}/*"),
            ));
            assert!(aws_permissions_boundary_covers(&variable, &variable).unwrap());
            assert!(
                aws_permissions_boundary_covers(&policy(allow("s3:*", "*")), &variable).unwrap()
            );
            let mut conditional = variable.clone();
            conditional["Statement"]["Condition"] = json!({
                "StringEquals": {"s3:prefix": [token, "shared/"]},
                "Bool": {"aws:SecureTransport": true},
                "NumericLessThanEquals": {"s3:max-keys": 10},
            });
            assert!(aws_permissions_boundary_covers(&conditional, &conditional).unwrap());
            assert!(aws_permissions_boundary_covers(&variable, &conditional).unwrap());
            assert!(!aws_permissions_boundary_covers(&conditional, &variable).unwrap());
        }
    }

    #[test]
    fn nested_malformed_conditions_cannot_be_hidden_by_other_grants() {
        let broad = policy(allow("*", "*"));
        for condition in [
            json!({}),
            json!({"": {"aws:SecureTransport": true}}),
            json!({"Bool": {}}),
            json!({"Bool": {"": true}}),
            json!({"Bool": true}),
            json!({"Bool": {"aws:SecureTransport": null}}),
            json!({"StringEquals": {"aws:RequestedRegion": []}}),
            json!({"StringEquals": {"aws:RequestedRegion": [["us-east-1"]]}}),
            json!({"StringEquals": {"aws:RequestedRegion": ["us-east-1", null]}}),
            json!({"StringEquals": {"aws:RequestedRegion": {"Ref": "Region"}}}),
            json!({"StringEquals": {"aws:RequestedRegion": {"Fn::Sub": "${AWS::Region}"}}}),
            json!({"${operator}": {"aws:RequestedRegion": "us-east-1"}}),
            json!({"StringEquals": {"${key}": "us-east-1"}}),
        ] {
            let mut statement = allow("s3:GetObject", "*");
            statement["Condition"] = condition;
            let malformed = policy(json!([allow("*", "*"), statement]));
            for (installed, required) in [
                (&malformed, &malformed),
                (&broad, &malformed),
                (&malformed, &broad),
            ] {
                assert_eq!(
                    aws_permissions_boundary_covers(installed, required)
                        .unwrap_err()
                        .code,
                    "INVALID_PERMISSION_SET",
                    "{malformed}"
                );
            }
        }
    }

    #[test]
    fn malformed_or_unrecognized_policies_fail_closed() {
        let valid = policy(allow("s3:GetObject", "*"));
        for malformed in [
            Value::Null,
            json!({"Version": "2012-10-17"}),
            json!({"Version": "2008-10-17", "Statement": []}),
            policy(json!(null)),
            policy(json!({"Effect": "Allow", "Action": [], "Resource": "*"})),
            policy(json!({"Effect": "Allow", "Action": [7], "Resource": "*"})),
            policy(
                json!({"Effect": "Allow", "Action": "*", "NotAction": "iam:*", "Resource": "*"}),
            ),
            policy(json!({"Effect": "Allow", "Action": "*", "Resource": "*", "Principal": "*"})),
            policy(json!({"Effect": "Allow", "Action": "*", "Resource": "*", "Condition": null})),
            policy(json!({"Effect": "Other", "Action": "*", "Resource": "*"})),
        ] {
            for (installed, required) in [(&malformed, &valid), (&valid, &malformed)] {
                assert_eq!(
                    aws_permissions_boundary_covers(installed, required)
                        .unwrap_err()
                        .code,
                    "INVALID_PERMISSION_SET"
                );
            }
        }
    }
}
