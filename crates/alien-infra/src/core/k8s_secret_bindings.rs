use crate::error::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};

/// Result of extracting secret references from a binding
pub struct BindingSecretExtraction {
    /// Environment variable definitions for secrets (name → SecretKeyRef)
    pub secret_env_vars: Vec<(String, String, String)>, // (env_var_name, secret_name, key)
    /// Modified binding JSON with SecretRef replaced by $(VAR) placeholders
    pub resolved_binding_json: String,
}

/// Extract SecretRef fields from a binding JSON and replace with $(VAR) placeholders
///
/// This function is Kubernetes-specific and handles the conversion of SecretRef objects
/// to environment variable placeholders that Kubernetes will expand at pod startup.
///
/// Process:
/// 1. Walks the JSON tree to find SecretRef objects (`{secretRef: {name, key}}`)
/// 2. Replaces each SecretRef with a placeholder like `$(ALIEN_BINDING_CACHE_PASSWORD)`
/// 3. Escapes existing user-provided `$(VAR)` patterns to `$$(VAR)` to prevent unwanted expansion
/// 4. Returns the modified JSON and a list of secret env vars to create as Kubernetes secretKeyRef
///
/// Example:
/// ```json
/// // Input:
/// {
///   "connectionUrl": "redis://$(REDIS_HOST):6379",
///   "password": {"secretRef": {"name": "redis-creds", "key": "password"}}
/// }
///
/// // Output JSON:
/// {
///   "connectionUrl": "redis://$$(REDIS_HOST):6379",  // User's var escaped
///   "password": "$(ALIEN_BINDING_CACHE_PASSWORD)"    // Our placeholder
/// }
///
/// // Output secret_env_vars:
/// [("ALIEN_BINDING_CACHE_PASSWORD", "redis-creds", "password")]
/// ```
pub fn extract_binding_secrets(
    binding_name: &str,
    binding_json: &serde_json::Value,
) -> Result<BindingSecretExtraction> {
    let mut secret_env_vars = Vec::new();
    let mut modified_json = binding_json.clone();

    // Walk the JSON tree to find SecretRef objects
    extract_secrets_recursive(
        &mut modified_json,
        &format!(
            "ALIEN_BINDING_{}_",
            binding_name.to_uppercase().replace('-', "_")
        ),
        &mut secret_env_vars,
        "",
    )?;

    // Serialize to JSON string
    let mut resolved_binding_json = serde_json::to_string(&modified_json)
        .into_alien_error()
        .context(ErrorData::ResourceConfigInvalid {
            message: "Failed to serialize binding JSON".to_string(),
            resource_id: Some(binding_name.to_string()),
        })?;

    // Escape existing $(VAR) patterns that aren't our placeholders
    // K8s expands $(VAR) but $$(VAR) becomes literal $(VAR)
    resolved_binding_json = escape_existing_var_references(&resolved_binding_json);

    Ok(BindingSecretExtraction {
        secret_env_vars,
        resolved_binding_json,
    })
}

/// Escape existing $(VAR) patterns to $$(VAR) to prevent Kubernetes expansion
/// Preserves our $(ALIEN_BINDING_...) placeholders unescaped
fn escape_existing_var_references(json_str: &str) -> String {
    let mut result = String::with_capacity(json_str.len());
    let mut chars = json_str.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' {
            if let Some(&'(') = chars.peek() {
                // Found $(, check if it's our placeholder or user's value
                let remaining: String = chars.clone().collect();

                if remaining.starts_with("(ALIEN_BINDING_") {
                    // Our placeholder - keep as-is
                    result.push('$');
                } else {
                    // User's value - escape it
                    result.push_str("$$");
                }
            } else {
                result.push(ch);
            }
        } else {
            result.push(ch);
        }
    }

    result
}

/// Recursively find and replace SecretRef objects with $(VAR) placeholders
fn extract_secrets_recursive(
    value: &mut serde_json::Value,
    env_var_prefix: &str,
    extractions: &mut Vec<(String, String, String)>,
    path: &str,
) -> Result<()> {
    match value {
        serde_json::Value::Object(map) => {
            // Check if this object is a SecretRef
            if let Some(secret_ref_obj) = map.get("secretRef") {
                if let (Some(name), Some(key)) = (
                    secret_ref_obj.get("name").and_then(|v| v.as_str()),
                    secret_ref_obj.get("key").and_then(|v| v.as_str()),
                ) {
                    // Generate env var name from path
                    let field_name = path.split('.').last().unwrap_or(path);
                    let env_var_name = format!(
                        "{}{}",
                        env_var_prefix,
                        field_name.to_uppercase().replace('-', "_")
                    );

                    extractions.push((env_var_name.clone(), name.to_string(), key.to_string()));

                    // Replace this value with placeholder (unescaped)
                    *value = serde_json::Value::String(format!("$({})", env_var_name));
                    return Ok(());
                }
            }

            // Recurse into object fields
            for (key, val) in map.iter_mut() {
                let new_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", path, key)
                };
                extract_secrets_recursive(val, env_var_prefix, extractions, &new_path)?;
            }
        }
        serde_json::Value::Array(arr) => {
            for (i, val) in arr.iter_mut().enumerate() {
                let new_path = format!("{}[{}]", path, i);
                extract_secrets_recursive(val, env_var_prefix, extractions, &new_path)?;
            }
        }
        _ => {}
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_binding_secrets_basic() {
        let binding = json!({
            "service": "redis",
            "host": "redis.internal",
            "port": 6379,
            "password": {
                "secretRef": {
                    "name": "redis-creds",
                    "key": "password"
                }
            }
        });

        let result = extract_binding_secrets("cache", &binding).unwrap();

        // Should have one secret env var
        assert_eq!(result.secret_env_vars.len(), 1);
        let (env_name, secret_name, key) = &result.secret_env_vars[0];
        assert_eq!(env_name, "ALIEN_BINDING_CACHE_PASSWORD");
        assert_eq!(secret_name, "redis-creds");
        assert_eq!(key, "password");

        // JSON should have placeholder
        assert!(result
            .resolved_binding_json
            .contains("$(ALIEN_BINDING_CACHE_PASSWORD)"));
        assert!(!result.resolved_binding_json.contains("secretRef"));
    }

    #[test]
    fn test_extract_binding_secrets_nested() {
        let binding = json!({
            "service": "postgres",
            "host": "db.internal",
            "credentials": {
                "username": "admin",
                "password": {
                    "secretRef": {
                        "name": "db-creds",
                        "key": "password"
                    }
                }
            }
        });

        let result = extract_binding_secrets("database", &binding).unwrap();

        assert_eq!(result.secret_env_vars.len(), 1);
        let (env_name, _, _) = &result.secret_env_vars[0];
        assert_eq!(env_name, "ALIEN_BINDING_DATABASE_PASSWORD");
    }

    #[test]
    fn test_extract_binding_secrets_multiple() {
        let binding = json!({
            "service": "custom",
            "apiKey": {
                "secretRef": {
                    "name": "api-creds",
                    "key": "key"
                }
            },
            "apiSecret": {
                "secretRef": {
                    "name": "api-creds",
                    "key": "secret"
                }
            }
        });

        let result = extract_binding_secrets("api", &binding).unwrap();

        assert_eq!(result.secret_env_vars.len(), 2);
        assert!(result
            .resolved_binding_json
            .contains("$(ALIEN_BINDING_API_APIKEY)"));
        assert!(result
            .resolved_binding_json
            .contains("$(ALIEN_BINDING_API_APISECRET)"));
    }

    #[test]
    fn test_escape_existing_var_references() {
        // User's value contains $(FOO) - should be escaped
        let input = r#"{"url":"redis://$(REDIS_HOST):6379"}"#;
        let result = escape_existing_var_references(input);
        assert_eq!(result, r#"{"url":"redis://$$(REDIS_HOST):6379"}"#);

        // Our placeholder - should NOT be escaped
        let input = r#"{"password":"$(ALIEN_BINDING_CACHE_PASSWORD)"}"#;
        let result = escape_existing_var_references(input);
        assert_eq!(result, r#"{"password":"$(ALIEN_BINDING_CACHE_PASSWORD)"}"#);
    }

    #[test]
    fn test_escape_mixed_var_references() {
        // Mix of user's vars and our placeholders
        let input = r#"{"url":"redis://$(HOST):6379","password":"$(ALIEN_BINDING_CACHE_PASSWORD)","db":"$(DB_NAME)"}"#;
        let result = escape_existing_var_references(input);
        assert_eq!(
            result,
            r#"{"url":"redis://$$(HOST):6379","password":"$(ALIEN_BINDING_CACHE_PASSWORD)","db":"$$(DB_NAME)"}"#
        );
    }

    #[test]
    fn test_extract_with_existing_var_references() {
        // Binding has both SecretRef AND existing $(VAR) in values
        let binding = json!({
            "service": "redis",
            "connectionUrl": "redis://$(REDIS_HOST):6379",
            "password": {
                "secretRef": {
                    "name": "redis-creds",
                    "key": "password"
                }
            }
        });

        let result = extract_binding_secrets("cache", &binding).unwrap();

        // Should extract the secret
        assert_eq!(result.secret_env_vars.len(), 1);

        // Should have our placeholder unescaped
        assert!(result
            .resolved_binding_json
            .contains("$(ALIEN_BINDING_CACHE_PASSWORD)"));

        // Should have user's var escaped
        assert!(result.resolved_binding_json.contains("$$(REDIS_HOST)"));

        // User's var should NOT be unescaped
        assert!(!result
            .resolved_binding_json
            .contains("redis://$(REDIS_HOST)"));
    }

    #[test]
    fn test_no_secrets_no_escaping_needed() {
        // No SecretRefs, no $(VAR) patterns
        let binding = json!({
            "service": "redis",
            "host": "redis.internal",
            "port": 6379
        });

        let result = extract_binding_secrets("cache", &binding).unwrap();

        assert_eq!(result.secret_env_vars.len(), 0);
        // Should be plain JSON, no changes
        let reparsed: serde_json::Value =
            serde_json::from_str(&result.resolved_binding_json).unwrap();
        assert_eq!(reparsed["host"], "redis.internal");
    }

    #[test]
    fn test_dollar_sign_not_var_reference() {
        // Edge case: $ not followed by (
        let binding = json!({
            "service": "custom",
            "price": "$100",
            "currency": "USD"
        });

        let result = extract_binding_secrets("api", &binding).unwrap();

        // $ without ( should remain unchanged
        assert!(result.resolved_binding_json.contains("$100"));
        assert!(!result.resolved_binding_json.contains("$$100"));
    }
}
