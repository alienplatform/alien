//! Commands service - SDK calls for command operations

use crate::tui::state::{CommandItem, CommandState};
use alien_platform_api::{types, Client, SdkResultExt};

/// Service for command SDK calls
#[derive(Clone)]
pub struct CommandsService {
    sdk: Client,
}

impl CommandsService {
    pub fn new(sdk: Client) -> Self {
        Self { sdk }
    }

    /// List all commands
    pub async fn list(&self) -> Result<Vec<CommandItem>, String> {
        let result = self
            .sdk
            .list_commands()
            .include(vec![types::ListCommandsIncludeItem::Deployment])
            .send()
            .await
            .into_sdk_error();
        match result {
            Ok(response) => {
                let items = response
                    .into_inner()
                    .items
                    .into_iter()
                    .map(|cmd| {
                        // Extract deployment name if present
                        // Note: SDK types don't have deployment_group_id or deployment_group yet
                        // They will be added when SDK is regenerated from updated OpenAPI spec
                        let deployment_name = cmd
                            .deployment
                            .as_ref()
                            .map(|d| d.name.as_str().to_string());

                        CommandItem {
                            id: cmd.id.to_string(),
                            name: cmd.name.to_string(),
                            state: state_from_api(cmd.state),
                            deployment_id: cmd.deployment_id.to_string(),
                            deployment_name,
                            deployment_group_id: None,
                            deployment_group_name: None,
                            created_at: cmd.created_at,
                        }
                    })
                    .collect();
                Ok(items)
            }
            Err(e) => Err(format!("Failed to load commands: {}", e)),
        }
    }
}

fn state_from_api(state: types::CommandListItemResponseState) -> CommandState {
    match state {
        types::CommandListItemResponseState::Pending => CommandState::Pending,
        types::CommandListItemResponseState::PendingUpload => CommandState::PendingUpload,
        types::CommandListItemResponseState::Dispatched => CommandState::Dispatched,
        types::CommandListItemResponseState::Succeeded => CommandState::Succeeded,
        types::CommandListItemResponseState::Failed => CommandState::Failed,
        types::CommandListItemResponseState::Expired => CommandState::Failed, // Treat expired as failed for display
    }
}
