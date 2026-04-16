//! OpenAPI documentation for the alien-manager API.

#[cfg(feature = "openapi")]
use utoipa::OpenApi;

/// OpenAPI documentation for the Alien Manager API
#[cfg(feature = "openapi")]
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Alien Manager API",
        version = "1.0.0",
        description = "Control plane for Alien applications. Manages deployments, releases, commands, and telemetry."
    ),
    paths(
        // Health
        crate::routes::health::health,
        // Identity
        crate::routes::whoami::whoami,
        // Deployments
        crate::routes::deployments::create_deployment,
        crate::routes::deployments::list_deployments,
        crate::routes::deployments::get_deployment,
        crate::routes::deployments::get_deployment_info,
        crate::routes::deployments::delete_deployment,
        crate::routes::deployments::retry_deployment,
        crate::routes::deployments::redeploy,
        // Releases
        crate::routes::releases::create_release,
        crate::routes::releases::get_release,
        crate::routes::releases::get_latest_release,
        // Deployment Groups
        crate::routes::deployment_groups::create_deployment_group,
        crate::routes::deployment_groups::list_deployment_groups,
        crate::routes::deployment_groups::get_deployment_group,
        crate::routes::deployment_groups::create_deployment_group_token,
        // Sync
        crate::routes::sync::acquire,
        crate::routes::sync::reconcile,
        crate::routes::sync::release,
        crate::routes::sync::agent_sync,
        crate::routes::sync::initialize,
        // Credentials
        crate::routes::credentials::resolve_credentials,
    ),
    components(schemas(
        // Deployment types
        crate::routes::deployments::CreateDeploymentRequest,
        crate::routes::deployments::CreateDeploymentResponse,
        crate::routes::deployments::DeploymentResponse,
        crate::routes::deployments::DeploymentGroupMinimal,
        crate::routes::deployments::ListDeploymentsResponse,
        crate::routes::deployments::DeploymentInfoResponse,
        // Release types
        crate::routes::releases::StackByPlatform,
        crate::routes::releases::CreateReleaseRequest,
        crate::routes::releases::GitMetadata,
        crate::routes::releases::ReleaseResponse,
        // Deployment group types
        crate::routes::deployment_groups::CreateDeploymentGroupRequest,
        crate::routes::deployment_groups::DeploymentGroupResponse,
        crate::routes::deployment_groups::ListDeploymentGroupsResponse,
        crate::routes::deployment_groups::CreateTokenResponse,
        // Sync types
        crate::routes::sync::AcquireRequest,
        crate::routes::sync::AcquireResponse,
        crate::routes::sync::AcquiredDeploymentResponse,
        crate::routes::sync::ReconcileRequest,
        crate::routes::sync::ReconcileResponse,
        crate::routes::sync::ReleaseRequest,
        crate::routes::sync::AgentSyncRequest,
        crate::routes::sync::AgentSyncResponse,
        crate::routes::sync::InitializeRequest,
        crate::routes::sync::InitializeResponse,
        // Credentials types
        crate::routes::credentials::ResolveCredentialsRequest,
        crate::routes::credentials::ResolveCredentialsResponse,
        // Identity types
        crate::routes::whoami::WhoamiResponse,
        // Health types
        crate::routes::health::HealthResponse,
        // Core types
        alien_core::Platform,
    )),
    tags(
        (name = "health", description = "Health check"),
        (name = "identity", description = "Authentication identity"),
        (name = "deployments", description = "Deployment lifecycle management"),
        (name = "releases", description = "Release management"),
        (name = "deployment-groups", description = "Deployment group management"),
        (name = "sync", description = "Agent sync and state reconciliation"),
        (name = "credentials", description = "Credential resolution for deployments"),
        (name = "telemetry", description = "OTLP telemetry ingestion"),
    )
)]
pub struct ApiDoc;
