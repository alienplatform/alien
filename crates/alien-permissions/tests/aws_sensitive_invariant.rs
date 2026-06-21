use alien_permissions::list_permission_set_ids;

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
                    assert!(
                        !SENSITIVE_IMPLICIT_ACTIONS.contains(&action.as_str()),
                        "{permission_set_id} AWS entry {index} grants sensitive action {action}"
                    );
                }
            }
        }
    }
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
