use alien_error::{Context, IntoAlienError};
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use url::Url;

use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::print_json;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Print executable integration examples",
    long_about = "Generate copyable examples using the active Alien environment. Secrets remain environment-variable references.",
    after_help = "EXAMPLES:
    alien examples ai-gateway --protocol openai-chat --model byo/claude-opus-5
    alien examples ai-gateway --protocol openai-responses --json
    alien examples ai-gateway --protocol anthropic-messages
    alien examples encryption-gateway --operation encrypt
    alien examples sandbox-gateway --operation exec --command 'python3 --version'"
)]
pub struct ExamplesArgs {
    /// Emit the example and metadata as JSON
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: ExampleCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ExampleCommand {
    /// Generate an AI Gateway request
    AiGateway {
        #[arg(long, value_enum, default_value_t = AiProtocol::OpenaiChat)]
        protocol: AiProtocol,
        /// Public model ID configured in the project
        #[arg(long, default_value = "byo/claude-opus-5")]
        model: String,
        /// Literal external ID. Omit to use the $CUSTOMER_ID environment variable.
        #[arg(long)]
        external_id: Option<String>,
    },
    /// Generate an Encryption Gateway request
    EncryptionGateway {
        #[arg(long, value_enum, default_value_t = EncryptionOperation::Encrypt)]
        operation: EncryptionOperation,
        /// Literal external ID. Omit to use the $CUSTOMER_ID environment variable.
        #[arg(long)]
        external_id: Option<String>,
        /// Stable key identifier for this data class
        #[arg(long, default_value = "customer-data")]
        key_id: String,
    },
    /// Generate a Sandbox Gateway request
    SandboxGateway {
        #[arg(long, value_enum, default_value_t = SandboxOperation::Create)]
        operation: SandboxOperation,
        /// Literal external ID. Omit to use the $CUSTOMER_ID environment variable.
        #[arg(long)]
        external_id: Option<String>,
        /// Shell command the exec example runs inside the sandbox
        #[arg(long, default_value = "echo hello")]
        command: String,
    },
}

#[derive(ValueEnum, Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiProtocol {
    OpenaiChat,
    OpenaiResponses,
    AnthropicMessages,
}

#[derive(ValueEnum, Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EncryptionOperation {
    Encrypt,
    Decrypt,
}

#[derive(ValueEnum, Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SandboxOperation {
    Create,
    Exec,
    Delete,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExampleOutput {
    service: &'static str,
    endpoint: String,
    command: String,
    required_environment: Vec<&'static str>,
}

pub fn examples_task(args: ExamplesArgs, ctx: ExecutionMode) -> Result<()> {
    let output = match args.command {
        ExampleCommand::AiGateway {
            protocol,
            model,
            external_id,
        } => ai_example(ctx.base_url(), protocol, &model, external_id.as_deref())?,
        ExampleCommand::EncryptionGateway {
            operation,
            external_id,
            key_id,
        } => encryption_example(ctx.base_url(), operation, external_id.as_deref(), &key_id)?,
        ExampleCommand::SandboxGateway {
            operation,
            external_id,
            command,
        } => sandbox_example(ctx.base_url(), operation, external_id.as_deref(), &command)?,
    };

    if args.json {
        print_json(&output)
    } else {
        println!("{}", output.command);
        Ok(())
    }
}

fn ai_example(
    api_base_url: String,
    protocol: AiProtocol,
    model: &str,
    external_id: Option<&str>,
) -> Result<ExampleOutput> {
    let endpoint = gateway_base_url(&api_base_url, "ai")?;
    let customer_header = external_id
        .map(shell_double_quote_fragment)
        .unwrap_or_else(|| "$CUSTOMER_ID".to_string());
    let (path, extra_header, body) = match protocol {
        AiProtocol::OpenaiChat => (
            "/v1/chat/completions",
            "",
            serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": "Hello"}]
            }),
        ),
        AiProtocol::OpenaiResponses => (
            "/v1/responses",
            "",
            serde_json::json!({"model": model, "input": "Hello"}),
        ),
        AiProtocol::AnthropicMessages => (
            "/v1/messages",
            "  -H \"Anthropic-Version: 2023-06-01\" \\\n",
            serde_json::json!({
                "model": model,
                "max_tokens": 1024,
                "messages": [{"role": "user", "content": "Hello"}]
            }),
        ),
    };
    let body = serde_json::to_string_pretty(&body)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to render AI Gateway example".to_string(),
        })?;
    let command = format!(
        "curl \"{endpoint}{path}\" \\\n  -H \"Authorization: Bearer $ALIEN_AI_API_KEY\" \\\n  -H \"X-Alien-External-ID: {customer_header}\" \\\n{extra_header}  -H \"Content-Type: application/json\" \\\n  -d {body}",
        body = shell_single_quote(&body)
    );
    Ok(ExampleOutput {
        service: "ai-gateway",
        endpoint,
        command,
        required_environment: if external_id.is_some() {
            vec!["ALIEN_AI_API_KEY"]
        } else {
            vec!["ALIEN_AI_API_KEY", "CUSTOMER_ID"]
        },
    })
}

fn encryption_example(
    api_base_url: String,
    operation: EncryptionOperation,
    external_id: Option<&str>,
    key_id: &str,
) -> Result<ExampleOutput> {
    let endpoint = gateway_base_url(&api_base_url, "encryption")?;
    let customer_header = external_id
        .map(shell_double_quote_fragment)
        .unwrap_or_else(|| "$CUSTOMER_ID".to_string());
    let (path, body, mut required_environment) = match operation {
        EncryptionOperation::Encrypt => (
            "/v1/encrypt",
            serde_json::json!({
                "key": {"keyId": key_id},
                "plaintext": "aGVsbG8="
            }),
            vec!["ALIEN_ENCRYPTION_API_KEY"],
        ),
        EncryptionOperation::Decrypt => (
            "/v1/decrypt",
            serde_json::json!({
                "key": {"keyId": key_id},
                "ciphertext": "$CIPHERTEXT"
            }),
            vec!["ALIEN_ENCRYPTION_API_KEY", "CIPHERTEXT"],
        ),
    };
    if external_id.is_none() {
        required_environment.push("CUSTOMER_ID");
    }
    let body = serde_json::to_string_pretty(&body)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to render Encryption Gateway example".to_string(),
        })?;
    let command = format!(
        "curl \"{endpoint}{path}\" \\\n  -H \"Authorization: Bearer $ALIEN_ENCRYPTION_API_KEY\" \\\n  -H \"X-Alien-External-ID: {customer_header}\" \\\n  -H \"Content-Type: application/json\" \\\n  -d {body}",
        body = shell_single_quote(&body).replace("$CIPHERTEXT", "'\"$CIPHERTEXT\"'")
    );
    Ok(ExampleOutput {
        service: "encryption-gateway",
        endpoint,
        command,
        required_environment,
    })
}

fn sandbox_example(
    api_base_url: String,
    operation: SandboxOperation,
    external_id: Option<&str>,
    shell_command: &str,
) -> Result<ExampleOutput> {
    let endpoint = gateway_base_url(&api_base_url, "sandbox")?;
    let customer_header = external_id
        .map(shell_double_quote_fragment)
        .unwrap_or_else(|| "$CUSTOMER_ID".to_string());
    let (method, path, body, mut required_environment) = match operation {
        SandboxOperation::Create => (
            "POST",
            "/v1/sandboxes".to_string(),
            Some(serde_json::json!({})),
            vec!["ALIEN_SANDBOX_API_KEY"],
        ),
        SandboxOperation::Exec => (
            "POST",
            "/v1/sandboxes/$SANDBOX_ID/exec".to_string(),
            Some(serde_json::json!({
                "command": "sh",
                "args": ["-c", shell_command],
                "timeoutMs": 30_000
            })),
            vec!["ALIEN_SANDBOX_API_KEY", "SANDBOX_ID"],
        ),
        SandboxOperation::Delete => (
            "DELETE",
            "/v1/sandboxes/$SANDBOX_ID".to_string(),
            None,
            vec!["ALIEN_SANDBOX_API_KEY", "SANDBOX_ID"],
        ),
    };
    if external_id.is_none() {
        required_environment.push("CUSTOMER_ID");
    }
    let body = body
        .map(|body| {
            serde_json::to_string_pretty(&body)
                .into_alien_error()
                .context(ErrorData::ConfigurationError {
                    message: "Failed to render Sandbox Gateway example".to_string(),
                })
        })
        .transpose()?;
    let body_lines = body
        .map(|body| {
            format!(
                " \\\n  -H \"Content-Type: application/json\" \\\n  -d {}",
                shell_single_quote(&body)
            )
        })
        .unwrap_or_default();
    let command = format!(
        "curl -X {method} \"{endpoint}{path}\" \\\n  -H \"Authorization: Bearer $ALIEN_SANDBOX_API_KEY\" \\\n  -H \"X-Alien-External-ID: {customer_header}\"{body_lines}"
    );
    Ok(ExampleOutput {
        service: "sandbox-gateway",
        endpoint,
        command,
        required_environment,
    })
}

fn gateway_base_url(api_base_url: &str, service: &str) -> Result<String> {
    let url =
        Url::parse(api_base_url)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: format!("Invalid platform base URL {api_base_url}"),
            })?;
    let host = url.host_str().ok_or_else(|| {
        alien_error::AlienError::new(ErrorData::ConfigurationError {
            message: format!("Platform base URL {api_base_url} has no hostname"),
        })
    })?;
    let gateway_host = host
        .strip_prefix("api.")
        .map(|suffix| format!("{service}.{suffix}"))
        .unwrap_or_else(|| format!("{service}.{host}"));
    let port = url
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    Ok(format!("{}://{gateway_host}{port}", url.scheme()))
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn shell_double_quote_fragment(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('$', "\\$")
        .replace('`', "\\`")
        .replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gateway_urls_follow_the_active_platform_environment() {
        assert_eq!(
            gateway_base_url("https://api.alien.localhost", "ai").unwrap(),
            "https://ai.alien.localhost"
        );
        assert_eq!(
            gateway_base_url("https://api.staging.alien.dev", "encryption").unwrap(),
            "https://encryption.staging.alien.dev"
        );
    }

    #[test]
    fn anthropic_example_contains_every_required_protocol_header() {
        let example = ai_example(
            "https://api.alien.dev".to_string(),
            AiProtocol::AnthropicMessages,
            "byo/claude-opus-5",
            None,
        )
        .unwrap();
        assert!(example.command.contains("/v1/messages"));
        assert!(example.command.contains("Anthropic-Version: 2023-06-01"));
        assert!(example
            .command
            .contains("X-Alien-External-ID: $CUSTOMER_ID"));
        assert!(example.command.contains("$ALIEN_AI_API_KEY"));
        assert!(!example.command.contains("\n+"));
    }

    #[test]
    fn decrypt_example_expands_ciphertext_without_breaking_json_quoting() {
        let example = encryption_example(
            "https://api.alien.dev".to_string(),
            EncryptionOperation::Decrypt,
            None,
            "customer-data",
        )
        .unwrap();
        assert!(example.command.contains("'\"$CIPHERTEXT\"'"));
        assert!(!example.command.contains("\n+"));
    }

    #[test]
    fn sandbox_exec_example_keeps_the_command_inside_one_json_string() {
        let example = sandbox_example(
            "https://api.alien.dev".to_string(),
            SandboxOperation::Exec,
            None,
            "echo 'it''s' && python3 -V",
        )
        .unwrap();
        assert!(example.command.starts_with(
            "curl -X POST \"https://sandbox.alien.dev/v1/sandboxes/$SANDBOX_ID/exec\""
        ));
        assert!(example.command.contains("$ALIEN_SANDBOX_API_KEY"));
        assert!(example
            .command
            .contains("X-Alien-External-ID: $CUSTOMER_ID"));
        assert_eq!(
            example.required_environment,
            vec!["ALIEN_SANDBOX_API_KEY", "SANDBOX_ID", "CUSTOMER_ID"]
        );
        assert!(!example.command.contains("\n+"));
    }

    #[test]
    fn sandbox_delete_example_sends_no_body() {
        let example = sandbox_example(
            "https://api.staging.alien.dev".to_string(),
            SandboxOperation::Delete,
            Some("org_123"),
            "echo hello",
        )
        .unwrap();
        assert!(example.command.starts_with(
            "curl -X DELETE \"https://sandbox.staging.alien.dev/v1/sandboxes/$SANDBOX_ID\""
        ));
        assert!(!example.command.contains("-d "));
        assert_eq!(
            example.required_environment,
            vec!["ALIEN_SANDBOX_API_KEY", "SANDBOX_ID"]
        );
    }

    #[test]
    fn literal_external_ids_are_safe_inside_the_header_quotes() {
        let example = ai_example(
            "https://api.alien.dev".to_string(),
            AiProtocol::OpenaiChat,
            "byo/example",
            Some("tenant-$HOME-\"quoted\""),
        )
        .unwrap();
        assert!(example
            .command
            .contains("X-Alien-External-ID: tenant-\\$HOME-\\\"quoted\\\""));
        assert!(!example.command.contains("X-Alien-External-ID: '"));
    }
}
