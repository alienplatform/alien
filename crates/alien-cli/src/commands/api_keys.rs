use std::num::NonZeroU64;

use alien_error::{Context, IntoAlienError};
use alien_platform_api::types::{
    CreateApiKeyRequest, CreateApiKeyRequestDescription, ProjectRole, ProjectScope,
    ProjectScopeType, Scope,
};
use alien_platform_api::SdkResultExt as _;
use clap::{Parser, Subcommand, ValueEnum};

use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::print_json;
use crate::ui::{dim_label, make_table, print_table, success_line};

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Manage API keys",
    after_help = "EXAMPLES:
    alien api-keys create --for ai-gateway --description production-backend
    alien api-keys create --for encryption-gateway --json
    alien api-keys list
    alien api-keys get key_abc123
    alien api-keys revoke key_abc123 --yes"
)]
pub struct ApiKeysArgs {
    /// Project to manage (defaults to the linked project)
    #[arg(long, global = true)]
    pub project: Option<String>,
    /// Emit stable machine-readable JSON
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: ApiKeysCommand,
}

impl ApiKeysArgs {
    pub fn wants_json_output(&self) -> bool {
        self.json
    }
}

#[derive(Subcommand, Debug, Clone)]
pub enum ApiKeysCommand {
    /// Create a least-privileged project key. The secret is returned only once.
    Create {
        /// Intended use; selects the corresponding project role
        #[arg(long = "for", value_enum)]
        purpose: ApiKeyPurpose,
        /// Human-readable key description
        #[arg(long)]
        description: Option<String>,
    },
    /// List project API keys without secrets
    #[command(visible_alias = "ls")]
    List,
    /// Show API-key metadata without its secret
    #[command(visible_aliases = ["describe", "show"])]
    Get { id: String },
    /// Revoke an API key
    #[command(visible_alias = "delete")]
    Revoke {
        id: String,
        /// Confirm revocation
        #[arg(long)]
        yes: bool,
    },
}

#[derive(ValueEnum, Debug, Clone, Copy)]
pub enum ApiKeyPurpose {
    AiGateway,
    EncryptionGateway,
    Deployments,
    RemoteBindings,
    ReadOnly,
}

pub async fn api_keys_task(args: ApiKeysArgs, ctx: ExecutionMode) -> Result<()> {
    let workspace = ctx.resolve_workspace_with_bootstrap(!args.json).await?;
    let (project_id, _) = ctx
        .resolve_project(args.project.as_deref(), !args.json)
        .await?;
    let client = ctx.sdk_client().await?;

    match args.command {
        ApiKeysCommand::Create {
            purpose,
            description,
        } => {
            let description = description
                .map(|value| {
                    CreateApiKeyRequestDescription::try_from(value)
                        .into_alien_error()
                        .context(ErrorData::ValidationError {
                            field: "description".to_string(),
                            message: "Invalid API-key description".to_string(),
                        })
                })
                .transpose()?;
            let response = client
                .create_api_key()
                .workspace(workspace.as_str())
                .body(&CreateApiKeyRequest {
                    description,
                    expires_at: None,
                    scope: Scope::ProjectScope(ProjectScope {
                        project_id,
                        role: purpose.role(),
                        type_: ProjectScopeType::Project,
                    }),
                })
                .send()
                .await
                .into_sdk_error()
                .context(ErrorData::ApiRequestFailed {
                    message: "Failed to create API key".to_string(),
                    url: None,
                })?
                .into_inner();
            if args.json {
                print_json(&response)?;
            } else {
                println!("{}", success_line("API key created."));
                println!("{} {}", dim_label("ID"), response.key_info.id.as_str());
                println!("{} {}", dim_label("Role"), response.key_info.role);
                println!();
                println!(
                    "{}",
                    dim_label("Save this secret now; it will not be shown again.")
                );
                println!("{}", response.api_key);
            }
        }
        ApiKeysCommand::List => {
            let mut items = Vec::new();
            let mut cursor: Option<String> = None;
            loop {
                let mut request = client
                    .list_api_keys()
                    .workspace(workspace.as_str())
                    .project(project_id.as_str())
                    .limit(NonZeroU64::new(100).expect("constant is non-zero"));
                if let Some(cursor) = cursor.as_deref() {
                    request = request.cursor(cursor);
                }
                let page = request
                    .send()
                    .await
                    .into_sdk_error()
                    .context(ErrorData::ApiRequestFailed {
                        message: "Failed to list API keys".to_string(),
                        url: None,
                    })?
                    .into_inner();
                items.extend(page.items);
                cursor = page.next_cursor;
                if cursor.is_none() {
                    break;
                }
            }
            if args.json {
                print_json(&items)?;
            } else if items.is_empty() {
                println!("{}", dim_label("No API keys found."));
            } else {
                let mut table = make_table(&["Description", "Role", "Prefix", "Status", "ID"]);
                for key in items {
                    table.add_row(vec![
                        key.description.unwrap_or_else(|| "-".to_string()),
                        key.role,
                        key.key_prefix,
                        if key.revoked_at.is_some() {
                            "revoked".to_string()
                        } else if key.enabled {
                            "active".to_string()
                        } else {
                            "disabled".to_string()
                        },
                        key.id.as_str().to_string(),
                    ]);
                }
                print_table(table);
            }
        }
        ApiKeysCommand::Get { id } => {
            let response = client
                .get_api_key()
                .id(id.as_str())
                .workspace(workspace.as_str())
                .send()
                .await
                .into_sdk_error()
                .context(ErrorData::ApiRequestFailed {
                    message: format!("Failed to get API key {id}"),
                    url: None,
                })?
                .into_inner();
            if args.json {
                print_json(&response)?;
            } else {
                println!("{} {}", dim_label("ID"), response.id.as_str());
                println!("{} {}", dim_label("Role"), response.role);
                println!("{} {}", dim_label("Prefix"), response.key_prefix);
                println!(
                    "{} {}",
                    dim_label("Status"),
                    if response.revoked_at.is_some() {
                        "revoked"
                    } else {
                        "active"
                    }
                );
            }
        }
        ApiKeysCommand::Revoke { id, yes } => {
            if !yes {
                return Err(alien_error::AlienError::new(ErrorData::ValidationError {
                    field: "yes".to_string(),
                    message: format!("Revoking {id} cannot be undone. Re-run with --yes."),
                }));
            }
            let response = client
                .revoke_api_key()
                .id(id.as_str())
                .workspace(workspace.as_str())
                .send()
                .await
                .into_sdk_error()
                .context(ErrorData::ApiRequestFailed {
                    message: format!("Failed to revoke API key {id}"),
                    url: None,
                })?
                .into_inner();
            if args.json {
                print_json(&response)?;
            } else {
                println!("{}", success_line("API key revoked."));
            }
        }
    }
    Ok(())
}

impl ApiKeyPurpose {
    fn role(self) -> ProjectRole {
        match self {
            Self::AiGateway => ProjectRole::ProjectAiGateway,
            Self::EncryptionGateway => ProjectRole::ProjectEncryption,
            Self::Deployments => ProjectRole::ProjectDeveloper,
            Self::RemoteBindings => ProjectRole::ProjectRemoteBindings,
            Self::ReadOnly => ProjectRole::ProjectViewer,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creation_uses_purpose_instead_of_internal_role_names() {
        let args = ApiKeysArgs::try_parse_from([
            "api-keys",
            "create",
            "--for",
            "ai-gateway",
            "--description",
            "production",
            "--json",
        ])
        .expect("purpose-based API key creation should parse");
        assert!(args.json);
        assert!(matches!(
            args.command,
            ApiKeysCommand::Create {
                purpose: ApiKeyPurpose::AiGateway,
                ..
            }
        ));
    }
}
