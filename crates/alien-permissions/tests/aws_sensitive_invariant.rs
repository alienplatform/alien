use alien_permissions::{list_permission_set_ids, SENSITIVE_MICROVM_ACTIONS};

const SENSITIVE_IMPLICIT_ACTIONS: &[&str] = &[
    "s3:GetObject",
    "s3:GetObjectVersion",
    "dynamodb:BatchGetItem",
    "dynamodb:GetItem",
    "dynamodb:Query",
    "dynamodb:Scan",
    "ssm:GetParameter",
    "ssm:GetParameters",
    "ssm:GetParametersByPath",
    "secretsmanager:GetSecretValue",
    "sqs:ReceiveMessage",
    "codebuild:BatchGetBuilds",
    "logs:GetLogEvents",
    "logs:FilterLogEvents",
];

#[test]
fn aws_implicit_management_sets_do_not_grant_sensitive_content() {
    for permission_set_id in list_permission_set_ids() {
        if !is_implicit_management_set(permission_set_id) {
            continue;
        }

        let permission_set = alien_permissions::get_permission_set(permission_set_id)
            .expect("permission set exists");
        let Some(aws_entries) = &permission_set.platforms.aws else {
            continue;
        };

        for (index, entry) in aws_entries.iter().enumerate() {
            if let Some(actions) = &entry.grant.actions {
                for action in actions {
                    // The MicroVM token/connect family is a session-content credential, held apart
                    // in one constant so the single-tenancy classifier and this invariant agree on
                    // what reaches a session.
                    assert!(
                        !SENSITIVE_IMPLICIT_ACTIONS.contains(&action.as_str())
                            && !SENSITIVE_MICROVM_ACTIONS.contains(&action.as_str()),
                        "{permission_set_id} AWS entry {index} grants sensitive action {action}"
                    );
                }
            }
        }
    }
}

/// The only permission sets that may mint a credential reaching inside a sandbox session.
///
/// `execute` serves a workload in the customer's own cloud; `remote-execute` serves a hosted
/// caller across the Remote Bindings boundary, and is resource-scoped for that reason.
const SESSION_REACHING_SANDBOX_SETS: &[&str] = &["sandbox/execute", "sandbox/remote-execute"];

#[test]
fn only_session_reaching_sandbox_sets_mint_microvm_auth_tokens() {
    let mut minting_sets = Vec::new();

    for permission_set_id in list_permission_set_ids() {
        let permission_set = alien_permissions::get_permission_set(permission_set_id)
            .expect("permission set exists");
        let Some(aws_entries) = &permission_set.platforms.aws else {
            continue;
        };
        let mints = aws_entries.iter().any(|entry| {
            entry.grant.actions.iter().flatten().any(|action| {
                action.starts_with("lambda:CreateMicrovm") && action.ends_with("AuthToken")
            })
        });
        if mints {
            minting_sets.push(permission_set_id);
        }
    }

    minting_sets.sort_unstable();
    let mut expected = SESSION_REACHING_SANDBOX_SETS.to_vec();
    expected.sort_unstable();
    assert_eq!(
        minting_sets, expected,
        "the sets that mint a MicroVM auth token changed; a new one reaches inside a session"
    );
}

fn is_implicit_management_set(permission_set_id: &str) -> bool {
    permission_set_id.ends_with("/heartbeat")
        || permission_set_id.ends_with("/management")
        || permission_set_id.ends_with("-management")
        || permission_set_id.ends_with("/provision")
}

#[test]
fn worker_heartbeat_does_not_grant_code_or_image_read() {
    let permission_set =
        alien_permissions::get_permission_set("worker/heartbeat").expect("permission set exists");
    let aws_entries = permission_set
        .platforms
        .aws
        .as_ref()
        .expect("worker heartbeat has AWS entries");

    let actions: Vec<&str> = aws_entries
        .iter()
        .flat_map(|entry| entry.grant.actions.iter().flatten().map(String::as_str))
        .collect();

    for sensitive_action in [
        "lambda:GetFunction",
        "ecr:BatchGetImage",
        "ecr:GetDownloadUrlForLayer",
    ] {
        assert!(
            !actions.contains(&sensitive_action),
            "worker/heartbeat should not grant {sensitive_action}"
        );
    }
}
