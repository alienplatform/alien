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

/// Azure resource IDs are case-insensitive, and ARM does not keep the casing
/// it was given. A managed identity read back says `resourcegroups`.
fn same_id(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// The rest of `id` after `prefix`, ignoring case like Azure does.
fn strip_id_prefix<'a>(id: &'a str, prefix: &str) -> Option<&'a str> {
    id.get(..prefix.len())
        .filter(|start| same_id(start, prefix))
        .map(|_| &id[prefix.len()..])
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
    // A registry created before setup owned a pull identity and grant has
    // neither reference. Such a setup keeps the stored pull until a setup
    // update, whether or not its payload named the pull principal.
    let saved_registry = resource
        .internal_state
        .clone()
        .and_then(|state| serde_json::from_value::<AzureArtifactRegistryController>(state).ok());
    if saved_registry.as_ref().is_some_and(|registry| {
        registry.pull_identity_resource_id.is_none() && registry.pull_role_assignment_id.is_none()
    }) {
        return Ok(None);
    }
    let WorkerCode::Image { image: source } = &worker.code else {
        return Err(invalid(
            worker,
            "Bounded registry import requires an immutable image",
        ));
    };
    let (host, source_image) = source
        .split_once('/')
        .ok_or_else(|| invalid(worker, "Image registry is missing"))?;
    let valid_repository = |repository: &str| {
        !repository.is_empty()
            && repository.split('/').all(|part| {
                !part.is_empty()
                    && part != "."
                    && part != ".."
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            })
    };
    // A Worker build names its image `repository:tag`. The import is by
    // digest, so a tag is resolved in the manager registry before it runs.
    enum SourceReference {
        Digest(String),
        Tag(String),
    }
    let (source_repository, reference) =
        if let Some((repository, digest)) = source_image.split_once("@sha256:") {
            if !valid_repository(repository)
                || digest.len() != 64
                || !digest.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(invalid(
                    worker,
                    "Bounded registry import requires a SHA-256 digest",
                ));
            }
            (repository, SourceReference::Digest(digest.to_string()))
        } else {
            let (repository, tag) = source_image.rsplit_once(':').ok_or_else(|| {
                invalid(
                    worker,
                    "Bounded registry import requires a SHA-256 digest or a tag",
                )
            })?;
            if !valid_repository(repository)
                || tag.is_empty()
                || tag.len() > 128
                || tag.contains('/')
                || !tag
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            {
                return Err(invalid(worker, "Image tag is invalid"));
            }
            (repository, SourceReference::Tag(tag.to_string()))
        };
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
    let registry = saved_registry
        .ok_or_else(|| invalid(worker, "Setup registry state is missing or invalid"))?;
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
    let identity_name = strip_id_prefix(identity_id, &identity_prefix)
        .filter(|s| !s.is_empty() && !s.contains('/'))
        .ok_or_else(setup)?;
    let assignment_prefix =
        format!("{registry_id}/providers/Microsoft.Authorization/roleAssignments/");
    if !strip_id_prefix(assignment_id, &assignment_prefix)
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
    if !current_identity
        .id
        .as_deref()
        .is_some_and(|id| same_id(id, identity_id))
        || current_identity
            .properties
            .and_then(|p| p.principal_id)
            .is_none_or(|id| !same_id(&id.to_string(), principal))
    {
        return Err(setup());
    }
    let authorization = ctx.service_provider.get_azure_authorization_client(azure)?;
    let assignment = authorization.get_role_assignment_by_id(assignment_id.to_owned()).await.context(ErrorData::ResourceConfigInvalid {
        message: "Cannot verify setup registry pull grant; update setup permissions if access is refused".into(), resource_id: Some(worker.id.clone()),
    })?;
    let properties = assignment.properties.ok_or_else(setup)?;
    if !assignment
        .id
        .as_deref()
        .is_some_and(|id| same_id(id, assignment_id))
        || !same_id(&properties.principal_id, principal)
        || !properties
            .scope
            .as_deref()
            .is_some_and(|scope| same_id(scope, &registry_id))
        || !properties
            .role_definition_id
            .to_ascii_lowercase()
            .ends_with("/roledefinitions/7f951dda-4ed3-4680-a7ca-43fe172d538d")
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
    if !live
        .id
        .as_deref()
        .is_some_and(|id| same_id(id, &registry_id))
        || registry.login_server.as_deref() != Some(login_server.as_str())
    {
        return Err(setup());
    }
    if let Some(current) = saved {
        if current.source != *source
            || !same_id(&current.registry_id, &registry_id)
            || !same_id(&current.identity_id, identity_id)
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
    let digest = match reference {
        SourceReference::Digest(digest) => digest,
        SourceReference::Tag(tag) => {
            let resolved = ctx
                .service_provider
                .resolve_registry_manifest_digest(
                    &format!("https://{host}"),
                    source_repository,
                    &tag,
                    &credential.token,
                )
                .await?;
            resolved
                .strip_prefix("sha256:")
                .filter(|digest| {
                    digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit())
                })
                .map(str::to_string)
                .ok_or_else(|| invalid(worker, "The registry returned an invalid image digest"))?
        }
    };
    // The import names the digest it resolved, so a tag that moves meanwhile
    // cannot change what is imported.
    let source_image = format!("{source_repository}@sha256:{digest}");
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use alien_azure_clients::authorization::MockAuthorizationApi;
    use alien_azure_clients::containerregistry::MockContainerRegistryApi;
    use alien_azure_clients::managed_identity::MockManagedIdentityApi;
    use alien_client_core::runtime_credentials::RuntimeCapability;
    use alien_core::{
        EnvironmentVariable, EnvironmentVariableType, EnvironmentVariablesSnapshot, Platform,
        Resource, StackResourceState,
    };
    use futures::executor::block_on;
    use serde_json::json;

    use super::*;
    use crate::core::{controller_test::SingleControllerExecutor, MockPlatformServiceProvider};
    use crate::worker::AzureWorkerController;

    const SUBSCRIPTION: &str = "12345678-1234-1234-1234-123456789012";
    const PRINCIPAL: &str = "0b5c1f7e-9d2a-4c1b-8e3f-2a6d4c8b1e90";
    const ASSIGNMENT: &str = "549f763a-e3d3-5825-a080-26402d449982";
    const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn registry_id() -> String {
        format!("/subscriptions/{SUBSCRIPTION}/resourceGroups/setup-rg/providers/Microsoft.ContainerRegistry/registries/setupacr")
    }

    fn assignment_id() -> String {
        format!(
            "{}/providers/Microsoft.Authorization/roleAssignments/{ASSIGNMENT}",
            registry_id()
        )
    }

    fn identity_id(resource_groups: &str) -> String {
        format!("/subscriptions/{SUBSCRIPTION}/{resource_groups}/setup-rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/artifacts-pull")
    }

    /// Imports one image with the setup pull identity saved as `stored`,
    /// while ARM reports the identity as `live`, the way the real API does.
    fn import(stored: &str, live: &str) -> (Result<Option<Duration>>, Option<ImportedImage>) {
        import_source(
            stored,
            live,
            &format!("test-manager.alien.dev/app@sha256:{DIGEST}"),
            false,
        )
    }

    /// [`import`] of an arbitrary source image. `resolves_tag` arms the manager
    /// registry lookup that turns the tag `app:v1` into [`DIGEST`].
    fn import_source(
        stored: &str,
        live: &str,
        image: &str,
        resolves_tag: bool,
    ) -> (Result<Option<Duration>>, Option<ImportedImage>) {
        let live = live.to_string();
        let mut identities = MockManagedIdentityApi::new();
        identities
            .expect_get_user_assigned_identity()
            .withf(|group, name| group == "setup-rg" && name == "artifacts-pull")
            .returning(move |_, _| {
                Ok(serde_json::from_value(json!({
                    "id": live,
                    "location": "eastus",
                    "properties": {"principalId": PRINCIPAL}
                }))
                .unwrap())
            });
        let mut authorization = MockAuthorizationApi::new();
        authorization
            .expect_get_role_assignment_by_id()
            .returning(|_| {
                Ok(serde_json::from_value(json!({
                    "id": assignment_id(),
                    "properties": {
                        "principalId": PRINCIPAL,
                        "scope": registry_id(),
                        "roleDefinitionId": format!("/subscriptions/{SUBSCRIPTION}/providers/Microsoft.Authorization/roleDefinitions/7f951dda-4ed3-4680-a7ca-43fe172d538d")
                    }
                }))
                .unwrap())
            });
        let imports = Arc::new(Mutex::new(Vec::new()));
        let seen = imports.clone();
        let mut registries = MockContainerRegistryApi::new();
        registries.expect_get_registry().returning(|_, _| {
            Ok(serde_json::from_value(json!({
                "id": registry_id(),
                "location": "eastus",
                "sku": {"name": "Basic"},
                "properties": {"loginServer": "setupacr.azurecr.io"}
            }))
            .unwrap())
        });
        registries
            .expect_import_image()
            .returning(move |group, name, parameters| {
                seen.lock().unwrap().push((
                    group.to_string(),
                    name.to_string(),
                    serde_json::to_value(parameters).unwrap(),
                ));
                Ok(OperationResult::Completed(()))
            });
        let identities = Arc::new(identities);
        let authorization = Arc::new(authorization);
        let registries = Arc::new(registries);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_azure_managed_identity_client()
            .returning(move |_| Ok(identities.clone()));
        provider
            .expect_get_azure_authorization_client()
            .returning(move |_| Ok(authorization.clone()));
        provider
            .expect_get_azure_container_registry_client()
            .returning(move |_| Ok(registries.clone()));
        if resolves_tag {
            provider
                .expect_resolve_registry_manifest_digest()
                .withf(|origin, repository, reference, token| {
                    origin == "https://test-manager.alien.dev"
                        && repository == "app"
                        && reference == "v1"
                        && token == "pull-token"
                })
                .returning(|_, _, _, _| Ok(format!("sha256:{DIGEST}")));
        }
        provider
            .expect_runtime_registry_pull_credential()
            .returning(|_| {
                Ok(Some(RuntimeCapability {
                    token: "pull-token".into(),
                    expires_at: chrono::Utc::now() + chrono::Duration::minutes(30),
                }))
            });
        let worker = Worker::new("api".to_string())
            .code(WorkerCode::Image {
                image: image.to_string(),
            })
            .permissions("default-profile".to_string())
            .build();
        let mut executor = block_on(
            SingleControllerExecutor::builder()
                .resource(worker.clone())
                .controller(AzureWorkerController::default())
                .platform(Platform::Azure)
                .service_provider(Arc::new(provider))
                .runtime_identity_audience("manager-one".into())
                .environment_variables(EnvironmentVariablesSnapshot {
                    variables: vec![EnvironmentVariable {
                        name: alien_core::ENV_ALIEN_DEPLOYMENT_ID.into(),
                        value: "dep-1".into(),
                        var_type: EnvironmentVariableType::Plain,
                        target_resources: None,
                    }],
                    hash: String::new(),
                    created_at: String::new(),
                })
                .build(),
        )
        .unwrap();
        let mut registry = StackResourceState::new_pending(
            ArtifactRegistry::RESOURCE_TYPE.to_string(),
            Resource::new(ArtifactRegistry::new("registry".to_string()).build()),
            Some(ResourceLifecycle::Frozen),
            vec![],
        );
        registry.status = ResourceStatus::Running;
        registry.internal_state = Some(
            serde_json::to_value(AzureArtifactRegistryController {
                registry_name: Some("setupacr".into()),
                resource_group_name: Some("setup-rg".into()),
                login_server: Some("setupacr.azurecr.io".into()),
                subscription_id: Some(SUBSCRIPTION.into()),
                // An empty `stored` is an older setup: a principal, and neither
                // the identity nor the role assignment reference.
                pull_identity_resource_id: (!stored.is_empty()).then(|| stored.into()),
                pull_principal_id: Some(PRINCIPAL.into()),
                pull_role_assignment_id: (!stored.is_empty()).then(assignment_id),
                ..Default::default()
            })
            .unwrap(),
        );
        executor.insert_saved_resource("registry", registry);
        let mut saved = None;
        let result = executor.with_context(|ctx| block_on(prepare(&mut saved, &worker, ctx)));
        let imports = imports.lock().unwrap();
        if matches!(result, Ok(Some(_))) {
            assert_eq!(imports.len(), 1);
            let (group, name, parameters) = &imports[0];
            assert_eq!((group.as_str(), name.as_str()), ("setup-rg", "setupacr"));
            assert_eq!(
                parameters["source"]["registryUri"],
                "test-manager.alien.dev"
            );
            assert_eq!(
                parameters["source"]["sourceImage"],
                format!("app@sha256:{DIGEST}")
            );
            assert_eq!(
                parameters["source"]["credentials"]["password"],
                "pull-token"
            );
            assert_eq!(
                parameters["untaggedTargetRepositories"],
                json!(["runtime/api"])
            );
        } else {
            assert!(imports.is_empty());
        }
        (result, saved)
    }

    /// Terraform saves `resourceGroups`; ARM reads the identity back with
    /// `resourcegroups`. Both name the same identity, so the import runs.
    #[test]
    fn import_accepts_the_identity_id_in_the_casing_azure_returns() {
        for (stored, live) in [
            (identity_id("resourceGroups"), identity_id("resourcegroups")),
            (identity_id("resourcegroups"), identity_id("resourcegroups")),
        ] {
            let (result, saved) = import(&stored, &live);
            assert_eq!(
                result.unwrap(),
                Some(Duration::from_secs(5)),
                "stored {stored}, live {live}"
            );
            let saved = saved.unwrap();
            assert!(saved.complete);
            assert_eq!(
                saved.image,
                format!("setupacr.azurecr.io/runtime/api@sha256:{DIGEST}")
            );
            assert_eq!(saved.registry_id, registry_id());
        }
    }

    /// A Worker build names its image `repository:tag`. The import is by digest,
    /// so the tag is resolved in the manager registry first, and the import
    /// names exactly that digest.
    #[test]
    fn import_resolves_a_tagged_image_to_its_digest() {
        let (result, saved) = import_source(
            &identity_id("resourceGroups"),
            &identity_id("resourcegroups"),
            "test-manager.alien.dev/app:v1",
            true,
        );
        assert_eq!(result.unwrap(), Some(Duration::from_secs(5)));
        assert_eq!(
            saved.unwrap().image,
            format!("setupacr.azurecr.io/runtime/api@sha256:{DIGEST}")
        );
    }

    /// An older setup supplies the pull principal but not the identity or the
    /// role assignment. Whatever the principal says, it keeps the stored pull
    /// until a setup update, as a setup with no pull fields at all does.
    #[test]
    fn an_older_setup_with_only_a_pull_principal_keeps_the_legacy_pull() {
        let old_payload = json!({
            "registryName": "setupacr",
            "resourceGroup": "setup-rg",
            "loginServer": "setupacr.azurecr.io",
            "subscriptionId": SUBSCRIPTION,
            "pushPrincipalId": PRINCIPAL,
            "pullPrincipalId": PRINCIPAL,
        });
        let data: alien_core::import::data::AzureArtifactRegistryImportData =
            serde_json::from_value(old_payload).expect("an older payload still imports");
        assert!(data.pull_identity_resource_id.is_none());
        assert!(data.pull_role_assignment_id.is_none());

        let (result, saved) = import_source(
            "",
            &identity_id("resourcegroups"),
            &format!("test-manager.alien.dev/app@sha256:{DIGEST}"),
            false,
        );
        assert_eq!(result.unwrap(), None, "no import, no setup-update error");
        assert!(saved.is_none());
    }

    /// Ignoring case must not accept a different identity.
    #[test]
    fn import_refuses_a_different_identity() {
        let other = identity_id("resourcegroups").replace("artifacts-pull", "other-pull");
        let (result, saved) = import(&identity_id("resourceGroups"), &other);
        let error = result.unwrap_err();
        assert!(
            error.to_string().contains("Setup update required"),
            "{error}"
        );
        assert!(saved.is_none());
    }
}
