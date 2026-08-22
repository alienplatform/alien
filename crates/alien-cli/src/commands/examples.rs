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
    alien examples encryption-gateway --operation encrypt"
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
