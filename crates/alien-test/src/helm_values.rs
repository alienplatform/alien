//! Helpers for writing Helm values files used by E2E tests.

use anyhow::Context;
use serde_json::Value;

pub(crate) const GHCR_PULL_SECRET_NAME: &str = "alien-e2e-ghcr-pull";

pub(crate) fn to_helm_values_yaml(values: &Value) -> anyhow::Result<String> {
    let yaml = serde_yaml::to_string(values).context("Failed to serialize Helm values")?;
    Ok(quote_yaml_1_1_boolean_words(&yaml))
}

pub(crate) fn runtime_image_pull_secrets(repository: &str) -> Option<Value> {
    if repository.starts_with("ghcr.io/") && ghcr_pull_token().is_some() {
        Some(serde_json::json!([{ "name": GHCR_PULL_SECRET_NAME }]))
    } else {
        None
    }
}

pub(crate) fn ghcr_pull_credentials() -> Option<(String, String)> {
    let token = ghcr_pull_token()?;
    let username = std::env::var("ALIEN_TEST_GHCR_USERNAME")
        .ok()
        .filter(|username| !username.is_empty())
        .unwrap_or_else(|| "x-access-token".to_string());
    Some((username, token))
}

fn ghcr_pull_token() -> Option<String> {
    std::env::var("ALIEN_TEST_GHCR_TOKEN")
        .ok()
        .filter(|token| !token.is_empty())
}

fn quote_yaml_1_1_boolean_words(yaml: &str) -> String {
    let mut out = String::with_capacity(yaml.len());
    for line in yaml.lines() {
        let quoted = if let Some(prefix) = line.strip_suffix(": on") {
            Some(format!("{prefix}: \"on\""))
        } else {
            line.strip_suffix(": off")
                .map(|prefix| format!("{prefix}: \"off\""))
        };
        out.push_str(quoted.as_deref().unwrap_or(line));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::to_helm_values_yaml;

    #[test]
    fn quotes_yaml_boolean_words_for_helm() {
        let yaml = to_helm_values_yaml(&serde_json::json!({
            "management": {
                "healthChecks": "on",
            },
            "stackSettings": {
                "heartbeats": "off",
            },
        }))
        .unwrap();

        assert!(yaml.contains("healthChecks: \"on\""));
        assert!(yaml.contains("heartbeats: \"off\""));
    }

    #[test]
    fn requires_pull_secret_for_ghcr_images_when_credentials_exist() {
        temp_env::with_var("ALIEN_TEST_GHCR_TOKEN", Some("token"), || {
            assert!(
                super::runtime_image_pull_secrets("ghcr.io/alienplatform/alien-agent").is_some()
            );
            assert!(super::runtime_image_pull_secrets("public.ecr.aws/example/agent").is_none());
        });
    }
}
