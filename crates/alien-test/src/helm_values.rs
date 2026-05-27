//! Helpers for writing Helm values files used by E2E tests.

use anyhow::Context;
use serde_json::Value;

pub(crate) fn to_helm_values_yaml(values: &Value) -> anyhow::Result<String> {
    let yaml = serde_yaml::to_string(values).context("Failed to serialize Helm values")?;
    Ok(quote_yaml_1_1_boolean_words(&yaml))
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
}
