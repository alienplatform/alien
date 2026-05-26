use alien_core::PermissionGrant;

pub(crate) fn entry_pascal_label(explicit: Option<&str>, grant: &PermissionGrant) -> String {
    to_pascal_case(&entry_kebab_label(explicit, grant))
}

pub(crate) fn entry_snake_label(explicit: Option<&str>, grant: &PermissionGrant) -> String {
    entry_kebab_label(explicit, grant).replace('-', "_")
}

pub(crate) fn entry_title_label(explicit: Option<&str>, grant: &PermissionGrant) -> String {
    entry_kebab_label(explicit, grant)
        .split('-')
        .filter(|part| !part.is_empty())
        .map(title_word)
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn has_explicit_label(explicit: Option<&str>) -> bool {
    explicit.is_some_and(|label| !sanitize_kebab(label).is_empty())
}

pub(crate) fn entry_description<'a>(
    explicit: Option<&'a str>,
    permission_set_description: &'a str,
) -> String {
    explicit
        .filter(|description| !description.trim().is_empty())
        .unwrap_or(permission_set_description)
        .to_string()
}

fn entry_kebab_label(explicit: Option<&str>, grant: &PermissionGrant) -> String {
    if let Some(label) = explicit {
        let sanitized = sanitize_kebab(label);
        if !sanitized.is_empty() {
            return sanitized;
        }
    }

    let values = grant
        .actions
        .as_ref()
        .or(grant.permissions.as_ref())
        .or(grant.residual_permissions.as_ref())
        .or(grant.data_actions.as_ref())
        .or(grant.predefined_roles.as_ref());
    let Some(values) = values else {
        return "permission-entry".to_string();
    };

    let first = values
        .first()
        .map(String::as_str)
        .unwrap_or("permission-entry");
    let first_label = action_label(first);
    let mut label = first_label.clone();
    if values.len() > 1 {
        if let Some(group_label) = provider_action_group_label(values) {
            return group_label;
        }
        label.push_str("-permissions");
    }
    label
}

fn provider_action_group_label(values: &[String]) -> Option<String> {
    let first_service = provider_service(values.first()?.as_str())?;
    if !values
        .iter()
        .all(|value| provider_service(value).as_deref() == Some(first_service.as_str()))
    {
        return None;
    }

    match first_service.as_str() {
        "acm" => Some("manage-tls-certificates".to_string()),
        "apigateway" => Some("manage-http-api-endpoints".to_string()),
        "ecr" => Some("read-ecr-images".to_string()),
        "ec2" | "compute" => Some("inspect-cloud-networking".to_string()),
        "events" | "cloudscheduler" => Some("manage-schedules".to_string()),
        "lambda" => Some("manage-lambda-functions".to_string()),
        "logs" | "logging" => Some("write-runtime-logs".to_string()),
        "pubsub" => Some("manage-pubsub-messaging".to_string()),
        "run" => Some("manage-cloud-run-services".to_string()),
        "s3" | "storage" => Some("manage-cloud-storage".to_string()),
        "secretmanager" => Some("manage-secret-manager-secrets".to_string()),
        "servicebus" => Some("manage-service-bus-queues".to_string()),
        _ => None,
    }
}

fn provider_service(value: &str) -> Option<String> {
    if let Some((service, _)) = value.split_once(':') {
        return Some(service.to_ascii_lowercase());
    }
    if let Some(rest) = value.strip_prefix("Microsoft.") {
        return rest
            .split(['/', '.'])
            .next()
            .filter(|service| !service.is_empty())
            .map(|service| service.to_ascii_lowercase());
    }
    value
        .split(['.', '/'])
        .next()
        .filter(|service| !service.is_empty())
        .map(|service| service.to_ascii_lowercase())
}

fn action_label(value: &str) -> String {
    let mut words = Vec::new();
    let mut current = String::new();

    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            if ch.is_ascii_uppercase() && !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        words.push(current);
    }

    let mut label = words
        .into_iter()
        .filter(|word| !word.is_empty())
        .take(6)
        .collect::<Vec<_>>()
        .join("-");
    if label.is_empty() {
        label.push_str("permission-entry");
    }
    label
}

fn sanitize_kebab(value: &str) -> String {
    let mut out = String::new();
    let mut last_was_dash = true;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash {
            out.push('-');
            last_was_dash = true;
        }
    }
    if out.ends_with('-') {
        out.pop();
    }
    out
}

fn to_pascal_case(value: &str) -> String {
    value
        .split('-')
        .filter(|part| !part.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<String>()
}

fn title_word(word: &str) -> String {
    match word {
        "acr" => "ACR".to_string(),
        "api" => "API".to_string(),
        "aws" => "AWS".to_string(),
        "azure" => "Azure".to_string(),
        "cloudrun" => "Cloud Run".to_string(),
        "ecr" => "ECR".to_string(),
        "gcp" => "GCP".to_string(),
        "http" => "HTTP".to_string(),
        "https" => "HTTPS".to_string(),
        "iam" => "IAM".to_string(),
        "oidc" => "OIDC".to_string(),
        "pubsub" => "Pub/Sub".to_string(),
        "s3" => "S3".to_string(),
        "sqs" => "SQS".to_string(),
        "tls" => "TLS".to_string(),
        "url" => "URL".to_string(),
        "urls" => "URLs".to_string(),
        "vnet" => "VNet".to_string(),
        "vpc" => "VPC".to_string(),
        _ => {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        }
    }
}
