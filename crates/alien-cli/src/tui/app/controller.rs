//! App controller - handles actions and updates state

use alien_platform_api::ClientInfo as _;
const LOCAL_DEV_PROJECT_ID: &str = "local-dev";
use tracing::{debug, error, warn};

use super::config::{AppConfig, AppMode};
use super::state::AppViewState;
use crate::tui::services::{AppServices, LogsService};
use crate::tui::state::{
    Action, ConnectionInfo, DeploymentDetailState, DeploymentStatus, LogsConnectionStatus, ViewId,
};

/// Controller that handles actions and updates state
pub struct AppController {
    /// Application configuration
    pub config: AppConfig,
    /// View state
    pub state: AppViewState,
    /// SDK services
    pub services: AppServices,
    /// Rebuild trigger sender (dev mode only)
    rebuild_tx: Option<tokio::sync::mpsc::Sender<()>>,
}

impl AppController {
    /// Create a new app controller
    pub fn new(mut config: AppConfig, services: AppServices) -> Self {
        // Get the actual SDK URL for display
        let sdk_url = config.sdk.baseurl().to_string();

        let connection = match config.mode {
            AppMode::Dev => ConnectionInfo::dev_with_url(sdk_url),
            AppMode::Platform => ConnectionInfo::platform_with_url(sdk_url),
        };

        // Extract rebuild_tx from config (ownership)
        let rebuild_tx = config.rebuild_tx.take();

        Self {
            state: AppViewState::new(config.mode, connection),
            config,
            services,
            rebuild_tx,
        }
    }

    /// Initialize the controller by loading essential data (deployment groups, etc.)
    /// This should be called after creation and before the first render
    pub async fn initialize(&mut self) {
        // Proactively load deployment groups so they're available for the "New Deployment" dialog
        // This fixes the bug where no groups appear until you visit the deployment groups tab
        self.load_deployment_groups_silent().await;
    }

    /// Handle an action returned by a view
    pub async fn handle_action(&mut self, action: Action) -> bool {
        match action {
            Action::None => false,

            Action::Quit => true,

            Action::NavigateToView(view) => {
                self.state.navigate_to(view);
                self.load_current_view().await;
                false
            }

            Action::NavigateToDeployment(deployment_id) => {
                // Find the deployment in the list to get its name, status, and deployment group
                let (deployment_name, deployment_status, dg_id, dg_name) = self
                    .state
                    .deployments
                    .items
                    .iter()
                    .find(|a| a.id == deployment_id)
                    .map(|a| {
                        (
                            a.name.clone(),
                            a.status,
                            a.deployment_group_id.clone(),
                            a.deployment_group_name.clone(),
                        )
                    })
                    .unwrap_or_else(|| {
                        (
                            deployment_id.clone(),
                            DeploymentStatus::Pending,
                            String::new(),
                            None,
                        )
                    });

                // In dev mode, set this as the active deployment for log association
                if self.config.mode == AppMode::Dev {
                    self.state.set_active_deployment(deployment_id.clone());
                }

                self.state
                    .navigate_to(ViewId::DeploymentDetail(deployment_id.clone()));

                // Create detail state with deployment group info
                let detail = DeploymentDetailState::new(
                    deployment_id.clone(),
                    deployment_name,
                    deployment_status,
                )
                .with_deployment_group(dg_id, dg_name);
                self.state.deployment_detail = Some(detail);

                // Load full deployment details (resources + metadata) from API
                self.load_deployment_detail(&deployment_id).await;
                false
            }

            Action::NavigateBack => {
                self.state.navigate_back();
                // Refresh the view we're returning to
                self.load_current_view().await;
                false
            }

            Action::Refresh => {
                self.load_current_view().await;
                false
            }

            Action::ShowError(msg) => {
                debug!("Error: {}", msg);
                false
            }

            Action::OpenNewDeploymentDialog => {
                // Refresh deployment groups to get latest (in case new ones were added)
                if let Ok(items) = self.services.deployment_groups.list().await {
                    self.state.deployment_groups.set_items(items);
                }

                // Convert deployment groups to dialog format
                let groups: Vec<_> = self
                    .state
                    .deployment_groups
                    .items
                    .iter()
                    .map(|dg| crate::tui::dialogs::DeploymentGroupInfo {
                        id: dg.id.clone(),
                        name: dg.name.clone(),
                    })
                    .collect();
                self.state.open_new_deployment_dialog(groups);
                false
            }

            Action::CreateDeployment {
                platform,
                name,
                deployment_group_id,
            } => {
                self.state.close_new_deployment_dialog();

                // Create deployment via API
                // Platform mode: project_id is always set (required by run_tui_dashboard)
                // Dev mode: project_id is None, use the default local dev project
                let project_id = self
                    .config
                    .project_id
                    .clone()
                    .unwrap_or_else(|| LOCAL_DEV_PROJECT_ID.to_string());

                match self
                    .services
                    .deployments
                    .create(&name, &project_id, Some(&deployment_group_id), &platform)
                    .await
                {
                    Ok(deployment) => {
                        // Navigate to the new deployment and load its details
                        let deployment_id = deployment.id.clone();
                        let deployment_status = deployment.status;
                        let dg_id = deployment.deployment_group_id.clone();
                        let dg_name = deployment.deployment_group_name.clone();

                        self.state
                            .navigate_to(ViewId::DeploymentDetail(deployment_id.clone()));
                        self.state.deployment_detail = Some(
                            DeploymentDetailState::new(
                                deployment.id,
                                deployment.name,
                                deployment_status,
                            )
                            .with_deployment_group(dg_id, dg_name),
                        );

                        // Load deployment details from API (will trigger status updates via periodic refresh)
                        self.load_deployment_detail(&deployment_id).await;
                    }
                    Err(e) => {
                        debug!("Failed to create deployment: {}", e);
                    }
                }
                false
            }

            Action::DeleteDeployment(deployment_id) => {
                // Delete via API
                if let Err(e) = self.services.deployments.delete(&deployment_id).await {
                    debug!("Failed to delete deployment: {}", e);
                }

                // Refresh the list
                self.load_deployments().await;
                false
            }

            Action::SwitchLogSource => {
                // Reconnect to the newly selected agent manager
                self.connect_to_log_source().await;
                false
            }

            Action::SearchLogs(query) => {
                // Execute DeepStore search (platform mode only)
                self.search_logs_deepstore(&query).await;
                false
            }

            Action::TriggerRebuild => {
                // Trigger rebuild in dev mode by sending signal to CLI
                if let Some(ref tx) = self.rebuild_tx {
                    if let Err(e) = tx.try_send(()) {
                        debug!("Failed to trigger rebuild: {}", e);
                    }
                }
                false
            }

            Action::ShowErrorDialog(error) => {
                // Open error dialog
                self.state.open_error_dialog(error);
                false
            }

            Action::NavigateToLogsFilteredByDeployment {
                deployment_id,
                deployment_name: _,
            } => {
                // Filter logs by deployment and navigate to logs view
                self.state.logs_view.filter_by_deployment(deployment_id);
                self.state.navigate_to(ViewId::Logs);
                self.load_current_view().await;
                false
            }

            Action::NavigateToCommandsFilteredByDeployment {
                deployment_id,
                deployment_name: _,
            } => {
                // Filter commands by deployment and navigate to commands view
                self.state.filter_commands_by_deployment(deployment_id);
                self.state.navigate_to(ViewId::Commands);
                self.load_current_view().await;
                false
            }

            Action::ClearFilters => {
                // Clear filters based on current view
                match &self.state.current_view {
                    ViewId::Logs => {
                        self.state.logs_view.clear_filters();
                    }
                    ViewId::Commands => {
                        self.state.clear_commands_filter();
                    }
                    _ => {}
                }
                false
            }
        }
    }

    /// Search logs using DeepStore query (platform mode)
    async fn search_logs_deepstore(&mut self, query: &str) {
        let Some(ref logs_service) = self.services.logs else {
            return;
        };

        let end_time = chrono::Utc::now();
        let start_time = end_time - chrono::Duration::hours(1); // Search last hour

        match logs_service
            .search_logs(query.to_string(), start_time, end_time, Some(500))
            .await
        {
            Ok(logs) => {
                debug!(count = logs.len(), query = %query, "Search returned logs");
                // Replace existing logs with search results
                self.state.logs.clear();
                for log in logs {
                    self.state.add_log(log);
                }
            }
            Err(e) => {
                warn!("DeepStore search failed: {}", e);
            }
        }
    }

    /// Load data for the current view (with loading state)
    pub async fn load_current_view(&mut self) {
        match &self.state.current_view {
            ViewId::Deployments => self.load_deployments().await,
            ViewId::DeploymentGroups => self.load_deployment_groups().await,
            ViewId::Commands => self.load_commands().await,
            ViewId::Releases => self.load_releases().await,
            ViewId::Packages => self.load_packages().await,
            ViewId::DeploymentDetail(id) => self.load_deployment_detail(&id.clone()).await,
            ViewId::Logs => self.load_logs().await,
        }
    }

    /// Refresh current view data (without showing loading state)
    /// Called periodically to keep data fresh
    pub async fn refresh_current_view(&mut self) {
        match &self.state.current_view {
            ViewId::Deployments => self.refresh_deployments().await,
            ViewId::DeploymentGroups => self.refresh_deployment_groups().await,
            ViewId::Commands => self.refresh_commands().await,
            ViewId::Releases => self.refresh_releases().await,
            ViewId::Packages => self.refresh_packages().await,
            ViewId::DeploymentDetail(id) => self.refresh_deployment_detail(&id.clone()).await,
            ViewId::Logs => self.refresh_logs().await,
        }
    }

    // === Initial loads (with loading state) ===

    async fn load_deployments(&mut self) {
        self.state.deployments.set_loading(true);
        match self.services.deployments.list().await {
            Ok(items) => {
                // In dev mode, set the first deployment as the active deployment
                // (there's typically only one deployment in dev mode)
                if self.config.mode == AppMode::Dev && !items.is_empty() {
                    self.state.set_active_deployment(items[0].id.clone());
                }
                self.state.deployments.set_items(items);
                // Update deployment name cache for log enrichment
                self.state.update_deployment_name_cache();
            }
            Err(e) => self.state.deployments.set_error(e),
        }
    }

    async fn load_deployment_groups(&mut self) {
        self.state.deployment_groups.set_loading(true);
        match self.services.deployment_groups.list().await {
            Ok(items) => self.state.deployment_groups.set_items(items),
            Err(e) => self.state.deployment_groups.set_error(e),
        }
    }

    /// Load deployment groups silently (without setting loading state)
    /// Used for proactive initialization
    async fn load_deployment_groups_silent(&mut self) {
        match self.services.deployment_groups.list().await {
            Ok(items) => self.state.deployment_groups.set_items(items),
            Err(e) => {
                // Silently fail - deployment groups will be empty but won't block UI
                debug!("Failed to preload deployment groups: {}", e);
            }
        }
    }

    async fn load_commands(&mut self) {
        self.state.commands.set_loading(true);
        match self.services.commands.list().await {
            Ok(items) => self.state.commands.set_items(items),
            Err(e) => self.state.commands.set_error(e),
        }
    }

    async fn load_releases(&mut self) {
        self.state.releases.set_loading(true);
        match self.services.releases.list().await {
            Ok(items) => self.state.releases.set_items(items),
            Err(e) => self.state.releases.set_error(e),
        }
    }

    async fn load_packages(&mut self) {
        self.state.packages.set_loading(true);
        match self.services.packages.list().await {
            Ok(items) => self.state.packages.set_items(items),
            Err(e) => self.state.packages.set_error(e),
        }
    }

    async fn load_deployment_detail(&mut self, deployment_id: &str) {
        // Fetch full deployment details including stack_state
        match self
            .services
            .deployments
            .get_with_resources(deployment_id)
            .await
        {
            Ok((deployment, resources, metadata)) => {
                if let Some(ref mut detail) = self.state.deployment_detail {
                    if detail.deployment_id == deployment_id {
                        detail.update_status(deployment.status);
                        detail.update_resources(resources);
                        detail.update_metadata(metadata);
                    }
                }
            }
            Err(e) => {
                debug!("Failed to load deployment detail: {}", e);
            }
        }
    }

    // === Background refreshes (without loading state) ===

    async fn refresh_deployments(&mut self) {
        if let Ok(items) = self.services.deployments.list().await {
            // Preserve selection
            let selected = self.state.deployments.selected;
            self.state.deployments.set_items(items);
            self.state.deployments.selected = selected;
            // Update deployment name cache for log enrichment
            self.state.update_deployment_name_cache();
        }
    }

    async fn refresh_deployment_groups(&mut self) {
        if let Ok(items) = self.services.deployment_groups.list().await {
            let selected = self.state.deployment_groups.selected;
            self.state.deployment_groups.set_items(items);
            self.state.deployment_groups.selected = selected;
        }
    }

    async fn refresh_commands(&mut self) {
        if let Ok(items) = self.services.commands.list().await {
            let selected = self.state.commands.selected;
            self.state.commands.set_items(items);
            self.state.commands.selected = selected;
        }
    }

    async fn refresh_releases(&mut self) {
        if let Ok(items) = self.services.releases.list().await {
            let selected = self.state.releases.selected;
            self.state.releases.set_items(items);
            self.state.releases.selected = selected;
        }
    }

    async fn refresh_packages(&mut self) {
        if let Ok(items) = self.services.packages.list().await {
            let selected = self.state.packages.selected;
            self.state.packages.set_items(items);
            self.state.packages.selected = selected;
        }
    }

    async fn refresh_deployment_detail(&mut self, deployment_id: &str) {
        // Refresh deployment detail data from API
        // Always update resources and metadata - the API has the authoritative state
        if let Ok((deployment, resources, metadata)) = self
            .services
            .deployments
            .get_with_resources(deployment_id)
            .await
        {
            if let Some(ref mut detail) = self.state.deployment_detail {
                if detail.deployment_id == deployment_id {
                    detail.update_status(deployment.status);
                    detail.update_resources(resources);
                    detail.update_metadata(metadata);
                }
            }
        }
    }

    async fn load_logs(&mut self) {
        // Skip if already connected (avoid re-initializing and duplicating logs)
        if self.services.logs.is_some() {
            debug!("Log service already connected, skipping initialization");
            return;
        }

        if self.config.mode == AppMode::Platform {
            // Production: Fetch available managers and connect to selected one
            // Fetch available managers
            match self.services.managers.list().await {
                Ok(managers) => {
                    debug!(count = managers.len(), "Loaded managers");
                    self.state.logs_view.set_managers(managers);
                }
                Err(e) => {
                    warn!("Failed to load managers: {}", e);
                    self.state.logs_view.connection_status = LogsConnectionStatus::Error(e);
                    return;
                }
            }

            // Connect to the selected manager
            self.connect_to_log_source().await;
        } else {
            // Dev mode: Query dev server directly
            self.connect_to_dev_log_source().await;
        }
    }

    /// Connect to the currently selected log source (manager)
    async fn connect_to_log_source(&mut self) {
        // Only relevant for platform mode
        if self.config.mode != AppMode::Platform {
            return;
        }

        // Stop existing log service if switching sources
        if let Some(ref logs_service) = self.services.logs {
            debug!("Stopping existing log service before switching");
            logs_service.stop_streaming().await;
        }

        let Some(manager) = self.state.logs_view.selected_manager().cloned() else {
            self.state.logs_view.connection_status = LogsConnectionStatus::Disconnected;
            self.services.logs = None;
            return;
        };

        let Some(ref url) = manager.url else {
            self.state.logs_view.connection_status =
                LogsConnectionStatus::Error("Manager has no URL".to_string());
            return;
        };

        self.state.logs_view.connection_status = LogsConnectionStatus::Connecting;

        // Get project ID from config - required for platform mode log streaming
        let Some(ref project_id) = self.config.project_id else {
            warn!("No project ID configured - cannot stream logs");
            self.state.logs_view.connection_status =
                LogsConnectionStatus::Error("No project configured".to_string());
            return;
        };

        let credentials = match self
            .services
            .managers
            .get_deepstore_token(&manager.id, project_id)
            .await
        {
            Ok(creds) => creds,
            Err(e) => {
                warn!("Failed to get DeepStore token: {}", e);
                self.state.logs_view.connection_status = LogsConnectionStatus::Error(e);
                return;
            }
        };

        // Create DeepstoreClient for type-safe API calls
        // For production: Manager acts as auth proxy, DeepStore control plane for SSE
        // For dev: Dev server implements both roles
        let control_plane_url = self
            .config
            .deepstore_control_plane_url
            .clone()
            .unwrap_or_else(|| url.clone());

        let query_token = credentials.token.clone();
        let deepstore_client = match deepstore_client::DeepstoreClient::builder()
            .control_plane_url(control_plane_url)
            .auth_proxy_url(url.clone())
            .query_token_provider(move || {
                let token = query_token.clone();
                async move { Ok(token) }
            })
            .build()
        {
            Ok(client) => client,
            Err(e) => {
                error!("Failed to build DeepStore client: {}", e);
                self.state.logs_view.connection_status = LogsConnectionStatus::Error(e.to_string());
                return;
            }
        };

        let logs_service = LogsService::new(deepstore_client, credentials.database_id);

        // Start streaming logs
        if let Err(e) = logs_service.start_streaming().await {
            warn!("Failed to start log streaming: {}", e);
            self.state.logs_view.connection_status = LogsConnectionStatus::Error(e);
            return;
        }

        self.services.logs = Some(logs_service);
        self.state.logs_view.connection_status = LogsConnectionStatus::Connected;
        // Note: initializing flag is cleared when first log arrives (in add_log)
        debug!(manager_id = %manager.id, "Connected to log source");
    }

    /// Connect to dev server's log API (dev mode)
    async fn connect_to_dev_log_source(&mut self) {
        debug!("Connecting to dev server log API");

        self.state.logs_view.connection_status = LogsConnectionStatus::Connecting;

        // Dev server is always at the SDK URL (localhost:9090)
        let base_url = self.config.sdk.baseurl();

        // Create DeepstoreClient pointing to dev server
        // Dev server implements the same API as Agent Manager
        let deepstore_client = match deepstore_client::DeepstoreClient::builder()
            .control_plane_url(base_url)
            .auth_proxy_url(format!("{}/v1", base_url))
            .query_token_provider(|| async { Ok("dev-token".to_string()) })
            .build()
        {
            Ok(client) => client,
            Err(e) => {
                error!("Failed to build DeepStore client for dev mode: {}", e);
                self.state.logs_view.connection_status = LogsConnectionStatus::Error(e.to_string());
                return;
            }
        };

        // Use "dev" as database_id for local development
        let logs_service = LogsService::new(deepstore_client, "dev".to_string());

        // Start streaming/polling logs
        if let Err(e) = logs_service.start_streaming().await {
            warn!("Failed to start log streaming in dev mode: {}", e);
            self.state.logs_view.connection_status = LogsConnectionStatus::Error(e);
            return;
        }

        self.services.logs = Some(logs_service);
        self.state.logs_view.connection_status = LogsConnectionStatus::Connected;
        // Note: initializing flag is cleared when first log arrives (in add_log)
        debug!("Connected to dev server log API");
    }

    async fn refresh_logs(&mut self) {
        // Poll for new logs from log service (both dev and platform modes)
        if let Some(ref logs_service) = self.services.logs {
            let new_logs = logs_service.poll_new_logs().await;
            for log in new_logs {
                self.state.add_log(log);
            }

            // Clear initializing flag after first poll attempt
            // (whether we got logs or not - connection is established)
            if self.state.logs_view.initializing {
                self.state.logs_view.initializing = false;
            }
        }
    }
}
