//! Setup-owned registry import for workloads using bounded runtime authority.

use std::time::Duration;

use alien_azure_clients::long_running_operation::{LongRunningOperation, OperationResult};
use alien_core::{ArtifactRegistry, ResourceLifecycle, ResourceStatus, Worker, WorkerCode};
use alien_error::{AlienError, Context};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    artifact_registry::AzureArtifactRegistryController,
    core::ResourceControllerContext,
    error::{ErrorData, Result},
};

/// Contains cloud object references only, never import source credentials.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImportedImage {
    pub source: String,
    pub image: String,
    pub registry_id: String,
    pub identity_id: String,
    pub operation: Option<LongRunningOperation>,
    pub complete: bool,
    pub applied: bool,
}

fn invalid(worker: &Worker, message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        message: message.into(),
        resource_id: Some(worker.id.clone()),
    })
}

/// One import or polling operation per controller step. Setup retains ownership
/// of the registry, identity and grant; runtime never creates or repairs them.
pub(crate) async fn prepare(
    saved: &mut Option<ImportedImage>,
    worker: &Worker,
    ctx: &ResourceControllerContext<'_>,
) -> Result<Option<Duration>> {
    if ctx.deployment_config.runtime_identity_audience.is_none() {
        return Ok(None);
    }
    let mut registries = ctx
        .state
        .resources
        .values()
        .filter(|resource| resource.resource_type == ArtifactRegistry::RESOURCE_TYPE.as_ref());
    // A setup from before the registry copy has no registry. It keeps pulling
    // with its stored deployment credential until a setup update adds one.
    let Some(resource) = registries.next() else {
        return Ok(None);
    };
    let WorkerCode::Image { image: source } = &worker.code else {
        return Err(invalid(
            worker,
            "Bounded registry import requires an immutable image",
        ));
    };
    let (host, source_image) = source
        .split_once('/')
        .ok_or_else(|| invalid(worker, "Image registry is missing"))?;
    let (_, digest) = source_image
        .split_once("@sha256:")
        .filter(|(repository, digest)| {
            !repository.is_empty()
                && repository.split('/').all(|part| {
                    !part.is_empty()
                        && part != "."
                        && part != ".."
                        && part
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
                })
                && digest.len() == 64
                && digest.bytes().all(|b| b.is_ascii_hexdigit())
        })
        .ok_or_else(|| invalid(worker, "Bounded registry import requires a SHA-256 digest"))?;
    let manager = ctx
        .deployment_config
        .manager_url
        .as_deref()
        .ok_or_else(|| invalid(worker, "Manager registry is not configured"))?;
    let manager = reqwest::Url::parse(manager)
        .map_err(|_| invalid(worker, "Manager registry URL is invalid"))?;
    let source_url = reqwest::Url::parse(&format!("https://{host}"))
        .map_err(|_| invalid(worker, "Source registry is invalid"))?;
    if manager.scheme() != "https"
        || manager.origin() != source_url.origin()
        || !source_url.username().is_empty()
        || source_url.password().is_some()
        || source_url.query().is_some()
        || source_url.fragment().is_some()
        || source_url.path() != "/"
        || !manager.username().is_empty()
        || manager.password().is_some()
        || manager.query().is_some()
        || manager.fragment().is_some()
        || manager.path() != "/"
    {
        return Err(invalid(
            worker,
            "Source registry must match the configured secure manager origin",
        ));
    }
    if registries.next().is_some()
        || resource.lifecycle != Some(ResourceLifecycle::Frozen)
        || resource.status != ResourceStatus::Running
    {
        return Err(invalid(
            worker,
            "Setup update required: one running setup-owned registry is required",
        ));
    }
    let registry: AzureArtifactRegistryController = serde_json::from_value(
        resource
            .internal_state
            .clone()
            .ok_or_else(|| invalid(worker, "Setup registry state is missing"))?,
    )
    .map_err(|_| invalid(worker, "Setup registry state is invalid"))?;
    let setup = || {
        invalid(
            worker,
            "Setup update required: registry pull identity and grant are missing or changed",
        )
    };
    let azure = ctx.get_azure_config()?;
    let group = registry.resource_group_name.as_deref().ok_or_else(setup)?;
    let name = registry.registry_name.as_deref().ok_or_else(setup)?;
    if registry.subscription_id.as_deref() != Some(azure.subscription_id.as_str()) {
        return Err(setup());
    }
    let registry_id = format!(
        "/subscriptions/{}/resourceGroups/{group}/providers/Microsoft.ContainerRegistry/registries/{name}",
        azure.subscription_id
    );
    let identity_id = registry
        .pull_identity_resource_id
        .as_deref()
        .ok_or_else(setup)?;
    let principal = registry
        .pull_principal_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(setup)?;
    let assignment_id = registry
        .pull_role_assignment_id
        .as_deref()
        .ok_or_else(setup)?;
    let identity_prefix = format!(
        "/subscriptions/{}/resourceGroups/{group}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/",
        azure.subscription_id
    );
    let identity_name = identity_id
        .strip_prefix(&identity_prefix)
        .filter(|s| !s.is_empty() && !s.contains('/'))
        .ok_or_else(setup)?;
    let assignment_prefix =
        format!("{registry_id}/providers/Microsoft.Authorization/roleAssignments/");
    if !assignment_id
        .strip_prefix(&assignment_prefix)
        .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok())
    {
        return Err(setup());
    }
    let identities = ctx
        .service_provider
        .get_azure_managed_identity_client(azure)?;
    let current_identity = identities.get_user_assigned_identity(group, identity_name).await.context(ErrorData::ResourceConfigInvalid {
        message: "Cannot verify setup-owned registry pull identity; update setup permissions if access is refused".into(), resource_id: Some(worker.id.clone()),
    })?;
    if current_identity.id.as_deref() != Some(identity_id)
        || current_identity
            .properties
            .and_then(|p| p.principal_id)
            .is_none_or(|id| id.to_string() != principal)
    {
        return Err(setup());
    }
    let authorization = ctx.service_provider.get_azure_authorization_client(azure)?;
    let assignment = authorization.get_role_assignment_by_id(assignment_id.to_owned()).await.context(ErrorData::ResourceConfigInvalid {
        message: "Cannot verify setup registry pull grant; update setup permissions if access is refused".into(), resource_id: Some(worker.id.clone()),
    })?;
    let properties = assignment.properties.ok_or_else(setup)?;
    if assignment.id.as_deref() != Some(assignment_id)
        || properties.principal_id != principal
        || properties.scope.as_deref() != Some(registry_id.as_str())
        || !properties
            .role_definition_id
            .ends_with("/roleDefinitions/7f951dda-4ed3-4680-a7ca-43fe172d538d")
        || properties.condition.is_some()
    {
        return Err(setup());
    }
    let client = ctx
        .service_provider
        .get_azure_container_registry_client(azure)?;
    let live =
        client
            .get_registry(group, name)
            .await
            .context(ErrorData::ResourceConfigInvalid {
                message: "Cannot verify setup registry; access errors do not indicate deletion"
                    .into(),
                resource_id: Some(worker.id.clone()),
            })?;
    let login_server = live
        .properties
        .and_then(|p| p.login_server)
        .ok_or_else(setup)?;
    if live.id.as_deref() != Some(registry_id.as_str())
        || registry.login_server.as_deref() != Some(login_server.as_str())
    {
        return Err(setup());
    }
    if let Some(current) = saved {
        if current.source != *source
            || current.registry_id != registry_id
            || current.identity_id != identity_id
        {
            if !current.complete {
                return Err(invalid(
                    worker,
                    "Wait for the previous image import before changing its source or setup identity",
                ));
            }
            *saved = None;
        }
    }
    if let Some(current) = saved {
        if current.complete {
            return Ok(None);
        }
        let operation = current
            .operation
            .as_ref()
            .ok_or_else(|| invalid(worker, "Image import operation handle is missing"))?;
        if client
            .poll_image_import(operation)
            .await
            .context(ErrorData::ResourceConfigInvalid {
                message: "Registry image import did not complete".into(),
                resource_id: Some(worker.id.clone()),
            })?
        {
            current.operation = None;
            current.complete = true;
        }
        return Ok(Some(Duration::from_secs(5)));
    }
    let mut identities = ctx
        .deployment_config
        .environment_variables
        .variables
        .iter()
        .filter(|v| v.name == alien_core::ENV_ALIEN_DEPLOYMENT_ID);
    let deployment = identities
        .next()
        .ok_or_else(|| invalid(worker, "Deployment identity is missing"))?;
    if identities.next().is_some()
        || deployment.value.is_empty()
        || deployment.target_resources.is_some()
        || deployment.var_type != alien_core::EnvironmentVariableType::Plain
    {
        return Err(invalid(
            worker,
            "Deployment identity must be unique and untargeted",
        ));
    }
    let credential = ctx
        .service_provider
        .runtime_registry_pull_credential(&deployment.value)
        .await?
        .ok_or_else(|| invalid(worker, "Bounded registry authority is unavailable"))?;
    let now = chrono::Utc::now();
    if credential.token.is_empty()
        || credential.expires_at <= now
        || credential.expires_at > now + chrono::Duration::hours(1)
    {
        return Err(invalid(
            worker,
            "Registry authority has an invalid deadline",
        ));
    }
    // Untagged immutable target: lost responses can safely retry the same content.
    if worker.id.is_empty()
        || !worker
            .id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
    {
        return Err(invalid(
            worker,
            "Worker ID cannot form an immutable registry repository",
        ));
    }
    let repository = format!("runtime/{}", worker.id);
    let parameters = serde_json::from_value(json!({"source":{"sourceImage":source_image,"registryUri":host,"credentials":{"username":"deployment","password":credential.token}},"untaggedTargetRepositories":[repository],"mode":"NoForce"}))
        .map_err(|_| invalid(worker, "Cannot encode registry import"))?;
    let result = client
        .import_image(group, name, &parameters)
        .await
        .context(ErrorData::ResourceConfigInvalid {
            message: "Image import failed; setup must grant the registry import action".into(),
            resource_id: Some(worker.id.clone()),
        })?;
    let (complete, operation) = match result {
        OperationResult::Completed(()) => (true, None),
        OperationResult::LongRunning(operation) => (false, Some(operation)),
    };
    *saved = Some(ImportedImage {
        source: source.clone(),
        image: format!("{login_server}/{repository}@sha256:{digest}"),
        registry_id,
        identity_id: identity_id.into(),
        complete,
        operation,
        applied: false,
    });
    Ok(Some(Duration::from_secs(5)))
}
