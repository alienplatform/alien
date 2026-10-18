//! Direct setup owns this capability and retains every native retirement receipt.

use std::collections::HashMap;

use alien_aws_clients::{
    iam::{CreateRoleRequest, CreateRoleTag, IamApi, Role},
    lambda::{
        CreateFunctionRequest, Environment, FunctionCode, FunctionConfiguration, LambdaApi,
        PublishVersionRequest, UpdateFunctionCodeRequest, UpdateFunctionConfigurationRequest,
    },
};
use alien_core::{
    aws_worker_gateway::{
        cleanup_revision, CLEANUP_FUNCTION_PREFIX, CLEANUP_HANDLER_ARCHIVE, CLEANUP_RUNTIME,
    },
    setup_resource_tags, InitialSetupAuthority, RemoteStackManagement, ResourceLifecycle,
};
use alien_error::{AlienError, Context};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{ErrorData, ResourceControllerContext, Result};

const POLICY: &str = "gateway-retirement";
const INVOKE_POLICY: &str = "gateway-retirement-invoke";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GatewaySetup {
    generation: String,
    revision: String,
    phase: Phase,
    role_id: Option<String>,
    role_arn: Option<String>,
    published_arn: Option<String>,
    #[serde(default)]
    function_absent: bool,
    #[serde(default)]
    policy_absent: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Phase {
    Role,
    RolePolicy,
    Function,
    Configuration,
    Code,
    Version,
    Invoke,
    Done,
}

impl GatewaySetup {
    /// The caller must checkpoint this intent before calling `advance`.
    pub(super) fn new() -> Self {
        Self {
            generation: uuid::Uuid::new_v4().to_string(),
            revision: cleanup_revision(),
            phase: Phase::Role,
            role_id: None,
            role_arn: None,
            published_arn: None,
            function_absent: false,
            policy_absent: false,
        }
    }

    pub(super) fn is_current(&self) -> bool {
        matches!(self.phase, Phase::Done) && self.revision == cleanup_revision()
    }

    pub(super) fn published_arn(&self) -> Option<&str> {
        self.published_arn.as_deref()
    }

    /// One retirement mutation per checkpoint; accepted deletion is never absence.
    pub(super) async fn delete_step(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<bool> {
        let boundary = Boundary::new(ctx, &self.generation)?;
        let config = ctx.get_aws_config()?;
        let lambda = ctx.service_provider.get_aws_lambda_client(config).await?;
        match lambda.get_function(&boundary.name).await {
            Ok(function) => {
                boundary.verify_tags(function.tags.as_ref())?;
                if self.function_absent {
                    return Err(AlienError::new(
                        boundary.error("Retired helper function reappeared"),
                    ));
                }
                match lambda.delete_function(&boundary.name, None).await {
                    Ok(()) => {}
                    Err(error) if super::aws::is_remote_not_found(&error) => {}
                    Err(error) => {
                        return Err(
                            error.context(boundary.error("Delete owned setup helper function"))
                        )
                    }
                }
                return Ok(false);
            }
            Err(error) if super::aws::is_remote_not_found(&error) => {}
            Err(error) => {
                return Err(error.context(boundary.error("Read retiring setup helper function")))
            }
        }
        if !self.function_absent {
            self.function_absent = true;
            return Ok(false);
        }
        let iam = ctx.service_provider.get_aws_iam_client(config).await?;
        let role = match iam.get_role(&boundary.name).await {
            Ok(response) => response.get_role_result.role,
            Err(error) if super::aws::is_remote_not_found(&error) => return Ok(true),
            Err(error) => {
                return Err(error.context(boundary.error("Read retiring setup helper role")))
            }
        };
        boundary.verify_role(&role, self.role_id.as_deref())?;
        if self.role_id.is_none() {
            self.role_id = Some(role.role_id);
            return Ok(false);
        }
        if !self.policy_absent {
            match iam.delete_role_policy(&boundary.name, POLICY).await {
                Ok(()) => {}
                Err(error) if super::aws::is_remote_not_found(&error) => {}
                Err(error) => {
                    return Err(error.context(boundary.error("Delete owned setup helper policy")))
                }
            }
            self.policy_absent = true;
            return Ok(false);
        }
        match iam.delete_role(&boundary.name).await {
            Ok(()) => {}
            Err(error) if super::aws::is_remote_not_found(&error) => {}
            Err(error) => {
                return Err(error.context(boundary.error("Delete owned setup helper role")))
            }
        }
        Ok(false)
    }

    pub(super) async fn advance(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        management_role: &str,
    ) -> Result<bool> {
        let boundary = Boundary::new(ctx, &self.generation)?;
        if self.revision != cleanup_revision() {
            self.revision = cleanup_revision();
            self.phase = Phase::RolePolicy;
            return Ok(false);
        }
        let config = ctx.get_aws_config()?;
        let iam = ctx.service_provider.get_aws_iam_client(config).await?;
        let role = match iam.get_role(&boundary.name).await {
            Ok(response) => Some(response.get_role_result.role),
            Err(error) if super::aws::is_remote_not_found(&error) => None,
            Err(error) => return Err(error.context(boundary.error("Read setup helper role"))),
        };
        if let Some(role) = &role {
            boundary.verify_role(role, self.role_id.as_deref())?;
        } else if !matches!(self.phase, Phase::Role) {
            return Err(AlienError::new(
                boundary.error("Recorded setup helper role is absent"),
            ));
        }
        if matches!(self.phase, Phase::Role) {
            let role = match role {
                Some(role) => role,
                None => iam.create_role(CreateRoleRequest::builder().role_name(boundary.name.clone())
                    .path("/setup/".into()).assume_role_policy_document(serde_json::json!({
                        "Version":"2012-10-17", "Statement":[{"Effect":"Allow",
                        "Principal":{"Service":"lambda.amazonaws.com"}, "Action":"sts:AssumeRole"}]
                    }).to_string())
                    .tags(boundary.tags.iter().map(|(key,value)| CreateRoleTag { key:key.clone(), value:value.clone() }).collect())
                    .build()).await.context(boundary.error("Create setup helper role"))?.create_role_result.role,
            };
            boundary.verify_role(&role, self.role_id.as_deref())?;
            self.role_id = Some(role.role_id);
            self.role_arn = Some(role.arn);
            self.phase = Phase::RolePolicy;
            return Ok(false);
        }
        let role_arn = self
            .role_arn
            .as_deref()
            .ok_or_else(|| AlienError::new(boundary.error("Missing setup role receipt")))?;
        if matches!(self.phase, Phase::RolePolicy) {
            let partition = role_arn
                .split(':')
                .nth(1)
                .ok_or_else(|| AlienError::new(boundary.error("Invalid setup role ARN")))?;
            let tasks = format!(
                "arn:{partition}:ecs:{}:{}:task-definition/{}-*-gateway-*:*",
                config.region, config.account_id, ctx.resource_prefix
            );
            iam.put_role_policy(&boundary.name, POLICY, &serde_json::json!({"Version":"2012-10-17", "Statement":[
                {"Effect":"Allow", "Action":["ecs:ListTagsForResource", "ecs:DeleteTaskDefinitions"], "Resource":tasks},
                {"Effect":"Allow", "Action":["ecs:DeregisterTaskDefinition"], "Resource":"*"}
            ]}).to_string()).await.context(boundary.error("Install setup helper policy"))?;
            self.phase = Phase::Function;
            return Ok(false);
        }
        let lambda = ctx.service_provider.get_aws_lambda_client(config).await?;
        let function = match lambda.get_function(&boundary.name).await {
            Ok(response) => {
                boundary.verify_tags(response.tags.as_ref())?;
                Some(response.configuration)
            }
            Err(error) if super::aws::is_remote_not_found(&error) => None,
            Err(error) => return Err(error.context(boundary.error("Read setup helper function"))),
        };
        if matches!(self.phase, Phase::Function) {
            if function.is_none() {
                lambda
                    .create_function(
                        CreateFunctionRequest::builder()
                            .function_name(boundary.name.clone())
                            .role(role_arn.to_string())
                            .package_type("Zip".into())
                            .runtime(CLEANUP_RUNTIME.into())
                            .handler("index.handler".into())
                            .timeout(60)
                            .environment(boundary.environment())
                            .tags(boundary.tags.clone())
                            .code(
                                FunctionCode::builder()
                                    .zip_file(STANDARD.encode(CLEANUP_HANDLER_ARCHIVE))
                                    .build(),
                            )
                            .publish(false)
                            .build(),
                    )
                    .await
                    .context(boundary.error("Create setup helper function"))?;
            }
            self.phase = Phase::Configuration;
            return Ok(false);
        }
        let function = function.ok_or_else(|| {
            AlienError::new(boundary.error("Recorded setup helper function is absent"))
        })?;
        if function.state.as_deref() == Some("Failed")
            || function.last_update_status.as_deref() == Some("Failed")
        {
            return Err(AlienError::new(
                boundary.error("Setup helper configuration failed"),
            ));
        }
        if function.state.as_deref() != Some("Active")
            || function.last_update_status.as_deref() == Some("InProgress")
        {
            return Ok(false);
        }
        let revision = function
            .revision_id
            .clone()
            .ok_or_else(|| AlienError::new(boundary.error("Missing Lambda revision guard")))?;
        match self.phase {
            Phase::Configuration => {
                if !boundary.matches_configuration(&function, role_arn) {
                    lambda
                        .update_function_configuration(
                            &boundary.name,
                            UpdateFunctionConfigurationRequest::builder()
                                .role(role_arn.to_string())
                                .runtime(CLEANUP_RUNTIME.into())
                                .handler("index.handler".into())
                                .timeout(60)
                                .environment(boundary.environment())
                                .revision_id(revision)
                                .build(),
                        )
                        .await
                        .context(boundary.error("Configure setup helper function"))?;
                    return Ok(false);
                }
                self.phase = Phase::Code;
            }
            Phase::Code => {
                if !boundary.matches_configuration(&function, role_arn) {
                    return Err(AlienError::new(
                        boundary.error("Setup helper configuration changed before code update"),
                    ));
                }
                if function.code_sha256.as_deref() != Some(boundary.code_hash.as_str()) {
                    lambda
                        .update_function_code(
                            &boundary.name,
                            UpdateFunctionCodeRequest::builder()
                                .zip_file(STANDARD.encode(CLEANUP_HANDLER_ARCHIVE))
                                .revision_id(revision)
                                .publish(false)
                                .build(),
                        )
                        .await
                        .context(boundary.error("Upload setup helper code"))?;
                    return Ok(false);
                }
                self.phase = Phase::Version;
            }
            Phase::Version => {
                if !boundary.matches_configuration(&function, role_arn)
                    || function.code_sha256.as_deref() != Some(boundary.code_hash.as_str())
                {
                    return Err(AlienError::new(
                        boundary.error("Setup helper changed before publication"),
                    ));
                }
                let mut marker = None;
                let mut markers = std::collections::HashSet::new();
                let mut published = Vec::new();
                loop {
                    let page = lambda
                        .list_versions_by_function(&boundary.name, marker)
                        .await
                        .context(boundary.error("Read published setup helper versions"))?;
                    for version in page.versions {
                        if version.version.as_deref().is_some_and(|v| {
                            !v.is_empty()
                                && !v.starts_with('0')
                                && v.bytes().all(|b| b.is_ascii_digit())
                        }) && boundary.matches_configuration(&version, role_arn)
                            && version.code_sha256.as_deref() == Some(boundary.code_hash.as_str())
                        {
                            published.push(version.function_arn.ok_or_else(|| {
                                AlienError::new(boundary.error("Published helper has no ARN"))
                            })?);
                        }
                    }
                    marker = page.next_marker;
                    if let Some(next) = &marker {
                        if !markers.insert(next.clone()) {
                            return Err(AlienError::new(
                                boundary.error("Repeated helper version page"),
                            ));
                        }
                    } else {
                        break;
                    }
                }
                match published.as_slice() {
                    [] => {
                        lambda
                            .publish_version(
                                &boundary.name,
                                PublishVersionRequest::builder()
                                    .code_sha256(boundary.code_hash.clone())
                                    .revision_id(revision)
                                    .description(self.revision.clone())
                                    .build(),
                            )
                            .await
                            .context(boundary.error("Publish guarded setup helper version"))?;
                        return Ok(false);
                    }
                    [arn] => {
                        self.published_arn = Some(arn.clone());
                        self.phase = Phase::Invoke;
                    }
                    _ => {
                        return Err(AlienError::new(
                            boundary.error("Ambiguous published helper versions"),
                        ))
                    }
                }
            }
            Phase::Invoke => {
                let arn = self.published_arn.as_ref().ok_or_else(|| {
                    AlienError::new(boundary.error("Missing published helper receipt"))
                })?;
                let management_arn = format!(
                    "arn:{}:iam::{}:role/{management_role}",
                    role_arn.split(':').nth(1).unwrap(),
                    config.account_id
                );
                alien_core::aws_worker_gateway::validate_cleanup_function_arn(
                    arn,
                    &management_arn,
                    management_role,
                    &config.region,
                )
                .context(boundary.error("Validate published setup helper boundary"))?;
                iam.put_role_policy(
                    management_role,
                    INVOKE_POLICY,
                    &serde_json::json!({"Version":"2012-10-17", "Statement":[{
                        "Effect":"Allow", "Action":["lambda:InvokeFunction"], "Resource":arn
                    }]})
                    .to_string(),
                )
                .await
                .context(boundary.error("Grant exact setup helper invocation"))?;
                self.phase = Phase::Done;
            }
            Phase::Done => return Ok(true),
            Phase::Role | Phase::RolePolicy | Phase::Function => unreachable!(),
        }
        Ok(false)
    }
}

struct Boundary {
    name: String,
    resource_id: String,
    prefix: String,
    tags: HashMap<String, String>,
    code_hash: String,
}

impl Boundary {
    fn new(ctx: &ResourceControllerContext<'_>, generation: &str) -> Result<Self> {
        let resource = ctx.desired_resource_config::<RemoteStackManagement>()?;
        ensure_setup_authority(ctx)?;
        let mut tags: HashMap<_, _> =
            setup_resource_tags(ctx.resource_prefix, &resource.id, "remote-stack-management")
                .into_iter()
                .collect();
        tags.insert("CreateAttempt".into(), generation.into());
        Ok(Self {
            name: format!("{CLEANUP_FUNCTION_PREFIX}{}", ctx.resource_prefix),
            resource_id: resource.id.clone(),
            prefix: ctx.resource_prefix.into(),
            tags,
            code_hash: STANDARD.encode(Sha256::digest(CLEANUP_HANDLER_ARCHIVE)),
        })
    }

    fn error(&self, message: &str) -> ErrorData {
        ErrorData::InfrastructureError {
            message: message.into(),
            operation: Some("setup_gateway_retirement".into()),
            resource_id: Some(self.resource_id.clone()),
        }
    }

    fn environment(&self) -> Environment {
        Environment::builder()
            .variables(HashMap::from([(
                "ALIEN_STACK_PREFIX".into(),
                self.prefix.clone(),
            )]))
            .build()
    }

    fn matches_configuration(&self, value: &FunctionConfiguration, role: &str) -> bool {
        value.role.as_deref() == Some(role)
            && value.runtime.as_deref() == Some(CLEANUP_RUNTIME)
            && value.handler.as_deref() == Some("index.handler")
            && value.timeout == Some(60)
            && value
                .environment
                .as_ref()
                .and_then(|env| env.variables.as_ref())
                == self.environment().variables.as_ref()
    }

    fn verify_tags(&self, tags: Option<&HashMap<String, String>>) -> Result<()> {
        if !tags.is_some_and(|tags| {
            self.tags
                .iter()
                .all(|(key, value)| tags.get(key) == Some(value))
        }) {
            return Err(AlienError::new(
                self.error("Setup helper native ownership differs"),
            ));
        }
        Ok(())
    }

    fn verify_role(&self, role: &Role, expected_id: Option<&str>) -> Result<()> {
        let mut tags = HashMap::new();
        for tag in role
            .tags
            .as_ref()
            .map(|tags| tags.member.as_slice())
            .unwrap_or_default()
        {
            if tags.insert(tag.key.clone(), tag.value.clone()).is_some() {
                return Err(AlienError::new(
                    self.error("Duplicate setup role ownership tags"),
                ));
            }
        }
        self.verify_tags(Some(&tags))?;
        if role.path != "/setup/"
            || role.role_name != self.name
            || role.role_id.is_empty()
            || expected_id.is_some_and(|id| id != role.role_id)
        {
            return Err(AlienError::new(
                self.error("Setup helper role identity differs"),
            ));
        }
        Ok(())
    }
}

pub(super) fn ensure_setup_authority(ctx: &ResourceControllerContext<'_>) -> Result<()> {
    let resource = ctx.desired_resource_config::<RemoteStackManagement>()?;
    let frozen = ctx
        .desired_stack
        .resources()
        .find(|(id, _)| *id == &resource.id)
        .map(|(_, entry)| entry.lifecycle == ResourceLifecycle::Frozen)
        .unwrap_or_else(|| {
            ctx.state
                .resources
                .get(&resource.id)
                .is_some_and(|entry| entry.lifecycle == Some(ResourceLifecycle::Frozen))
        });
    if ctx.initial_setup_authority != InitialSetupAuthority::DirectSetup || !frozen {
        return Err(AlienError::new(ErrorData::SetupRequired {
            message:
                "Gateway retirement capability requires administrator setup of frozen management"
                    .into(),
            resource_id: Some(resource.id.clone()),
        }));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{AwsRemoteStackManagementController, AwsRemoteStackManagementState};
    use super::*;
    use crate::{core::controller_test::SingleControllerExecutor, MockPlatformServiceProvider};
    use alien_aws_clients::{
        iam::{GetRoleResponse, GetRoleResult, MockIamApi},
        lambda::{GetFunctionResponse, ListVersionsByFunctionResponse, MockLambdaApi},
    };
    use alien_core::Platform;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    async fn executor(
        controller: AwsRemoteStackManagementController,
        provider: Arc<MockPlatformServiceProvider>,
    ) -> SingleControllerExecutor {
        SingleControllerExecutor::builder()
            .resource(RemoteStackManagement::new("management".into()).build())
            .controller(controller)
            .platform(Platform::Aws)
            .resource_prefix("example")
            .resource_lifecycle(ResourceLifecycle::Frozen)
            .initial_setup_authority(InitialSetupAuthority::DirectSetup)
            .service_provider(provider)
            .build()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn lost_publication_reply_recovers_native_version_after_controller_reload() {
        let mut setup = GatewaySetup::new();
        setup.phase = Phase::Version;
        setup.role_id = Some("owned-role-id".into());
        let role_arn = "arn:aws:iam::123456789012:role/setup/_gateway-cleanup-example";
        let version_arn =
            "arn:aws:lambda:us-east-1:123456789012:function:_gateway-cleanup-example:7";
        setup.role_arn = Some(role_arn.into());
        let mut tags: HashMap<_, _> =
            setup_resource_tags("example", "management", "remote-stack-management")
                .into_iter()
                .collect();
        tags.insert("CreateAttempt".into(), setup.generation.clone());
        let role_tags: Vec<_> = tags
            .iter()
            .map(|(key, value)| serde_json::json!({"Key":key,"Value":value}))
            .collect();
        let mut iam = MockIamApi::new();
        iam.expect_get_role()
            .withf(|name| name == "_gateway-cleanup-example")
            .times(4)
            .returning(move |_| {
                Ok(GetRoleResponse {
                    get_role_result: GetRoleResult {
                        role: serde_json::from_value(serde_json::json!({"Path":"/setup/",
                    "RoleName":"_gateway-cleanup-example", "RoleId":"owned-role-id", "Arn":role_arn,
                    "CreateDate":"2026-01-01T00:00:00Z", "Tags":{"member":role_tags}}))
                        .unwrap(),
                    },
                })
            });
        iam.expect_put_role_policy()
            .times(1)
            .withf(move |role, policy, body| {
                role == "example-management"
                    && policy == INVOKE_POLICY
                    && serde_json::from_str::<serde_json::Value>(body).unwrap()["Statement"][0]
                        ["Resource"]
                        == version_arn
            })
            .returning(|_, _, _| Ok(()));
        let current = FunctionConfiguration {
            function_name: Some("_gateway-cleanup-example".into()),
            function_arn: Some(
                "arn:aws:lambda:us-east-1:123456789012:function:_gateway-cleanup-example".into(),
            ),
            version: Some("$LATEST".into()),
            revision_id: Some("native-revision".into()),
            state: Some("Active".into()),
            last_update_status: Some("Successful".into()),
            role: Some(role_arn.into()),
            runtime: Some(CLEANUP_RUNTIME.into()),
            handler: Some("index.handler".into()),
            timeout: Some(60),
            code_sha256: Some(STANDARD.encode(Sha256::digest(CLEANUP_HANDLER_ARCHIVE))),
            environment: Some(
                Environment::builder()
                    .variables(HashMap::from([(
                        "ALIEN_STACK_PREFIX".into(),
                        "example".into(),
                    )]))
                    .build(),
            ),
            ..Default::default()
        };
        let mut published = current.clone();
        published.version = Some("7".into());
        published.function_arn = Some(version_arn.into());
        let mut lambda = MockLambdaApi::new();
        lambda.expect_get_function().times(4).returning(move |_| {
            Ok(GetFunctionResponse {
                configuration: current.clone(),
                tags: Some(tags.clone()),
            })
        });
        let publication_exists = Arc::new(AtomicBool::new(false));
        let exists = publication_exists.clone();
        lambda
            .expect_list_versions_by_function()
            .times(2)
            .returning(move |_, marker| {
                assert!(marker.is_none());
                Ok(ListVersionsByFunctionResponse {
                    versions: if exists.load(Ordering::SeqCst) {
                        vec![published.clone()]
                    } else {
                        vec![]
                    },
                    next_marker: None,
                })
            });
        let exists = publication_exists.clone();
        lambda
            .expect_publish_version()
            .times(1)
            .return_once(move |_, request| {
                assert_eq!(request.revision_id, "native-revision");
                assert_eq!(
                    request.code_sha256,
                    STANDARD.encode(Sha256::digest(CLEANUP_HANDLER_ARCHIVE))
                );
                exists.store(true, Ordering::SeqCst);
                Err(AlienError::new(alien_client_core::ErrorData::Timeout {
                    message: "publication response lost".into(),
                }))
            });
        let iam = Arc::new(iam);
        let lambda = Arc::new(lambda);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_iam_client()
            .returning(move |_| Ok(iam.clone()));
        provider
            .expect_get_aws_lambda_client()
            .returning(move |_| Ok(lambda.clone()));
        let provider = Arc::new(provider);
        let mut controller = AwsRemoteStackManagementController::mock_ready("example-management");
        controller.state = AwsRemoteStackManagementState::CreatingGatewaySetup;
        controller.gateway_setup = Some(setup);
        let mut first = executor(controller, provider.clone()).await;
        first
            .step()
            .await
            .expect_err("lost publication reply must fail the step");
        let restored: AwsRemoteStackManagementController = serde_json::from_value(
            serde_json::to_value(
                first
                    .internal_state::<AwsRemoteStackManagementController>()
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(restored.worker_gateway_cleanup_function_arn.is_none());
        drop(first);
        let mut resumed = executor(restored, provider).await;
        for _ in 0..3 {
            resumed.step().await.unwrap();
        }
        let controller = resumed
            .internal_state::<AwsRemoteStackManagementController>()
            .unwrap();
        assert_eq!(controller.state, AwsRemoteStackManagementState::Ready);
        assert_eq!(
            controller.worker_gateway_cleanup_function_arn.as_deref(),
            Some(version_arn)
        );
        assert!(controller.gateway_setup.as_ref().unwrap().is_current());
    }

    #[tokio::test]
    async fn helper_read_failure_retains_cleanup_ownership_for_executor_retry() {
        let setup = GatewaySetup::new();
        let generation = setup.generation.clone();
        let mut lambda = MockLambdaApi::new();
        lambda.expect_get_function().times(1).returning(|_| {
            Err(AlienError::new(alien_client_core::ErrorData::Timeout {
                message: "native ownership read timed out".into(),
            }))
        });
        let lambda = Arc::new(lambda);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_lambda_client()
            .returning(move |_| Ok(lambda.clone()));
        let mut controller = AwsRemoteStackManagementController::mock_ready("example-management");
        controller.state = AwsRemoteStackManagementState::DeleteStart;
        controller.gateway_setup = Some(setup);
        let mut executor = executor(controller, Arc::new(provider)).await;
        assert_eq!(
            executor.step().await.unwrap_err().code,
            "RESOURCE_CLEANUP_INCOMPLETE"
        );
        let restored: AwsRemoteStackManagementController = serde_json::from_value(
            serde_json::to_value(
                executor
                    .internal_state::<AwsRemoteStackManagementController>()
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(restored.state, AwsRemoteStackManagementState::DeleteStart);
        let ownership = restored.gateway_setup.unwrap();
        assert_eq!(ownership.generation, generation);
        assert!(!ownership.function_absent);
        assert!(!ownership.policy_absent);
    }

    #[tokio::test]
    async fn setup_capability_refuses_imported_or_live_owners_before_provider_access() {
        for (authority, lifecycle) in [
            (
                InitialSetupAuthority::ImportedHandoff,
                ResourceLifecycle::Frozen,
            ),
            (
                InitialSetupAuthority::ImportedHandoff,
                ResourceLifecycle::Live,
            ),
            (InitialSetupAuthority::DirectSetup, ResourceLifecycle::Live),
        ] {
            let mut controller =
                AwsRemoteStackManagementController::mock_ready("example-management");
            controller.state = AwsRemoteStackManagementState::CreatingGatewaySetup;
            controller.gateway_setup = Some(GatewaySetup::new());
            let mut executor = SingleControllerExecutor::builder()
                .resource(RemoteStackManagement::new("management".into()).build())
                .controller(controller)
                .platform(Platform::Aws)
                .resource_prefix("example")
                .resource_lifecycle(lifecycle)
                .initial_setup_authority(authority)
                .service_provider(Arc::new(MockPlatformServiceProvider::new()))
                .build()
                .await
                .unwrap();
            assert_eq!(
                executor.step().await.unwrap_err().code,
                "DEPLOYMENT_SETUP_REQUIRED"
            );
        }
    }

    #[tokio::test]
    async fn native_generation_and_role_id_survive_checkpoint_and_reject_replacements() {
        let mut setup = GatewaySetup::new();
        setup.role_id = Some("owned-role-id".into());
        let checkpoint = serde_json::to_vec(&setup).unwrap();
        let restored: GatewaySetup = serde_json::from_slice(&checkpoint).unwrap();
        assert_eq!(restored.generation, setup.generation);
        let executor = SingleControllerExecutor::builder()
            .resource(RemoteStackManagement::new("management".into()).build())
            .controller(AwsRemoteStackManagementController::mock_ready(
                "example-management",
            ))
            .platform(Platform::Aws)
            .resource_prefix("example")
            .resource_lifecycle(ResourceLifecycle::Frozen)
            .initial_setup_authority(InitialSetupAuthority::DirectSetup)
            .service_provider(Arc::new(MockPlatformServiceProvider::new()))
            .build()
            .await
            .unwrap();
        executor.with_context(|ctx| {
            let boundary = Boundary::new(ctx, &restored.generation).unwrap();
            for case in ["owned", "foreign-generation", "replacement-id", "runtime-path", "duplicate-tag"] {
                let mut tags: Vec<_> = boundary.tags.iter().map(|(key,value)| serde_json::json!({"Key":key,"Value":value})).collect();
                if case == "foreign-generation" {
                    tags.iter_mut().find(|tag| tag["Key"] == "CreateAttempt").unwrap()["Value"] = serde_json::json!(uuid::Uuid::new_v4().to_string());
                }
                if case == "duplicate-tag" { tags.push(tags[0].clone()); }
                let role: Role = serde_json::from_value(serde_json::json!({
                    "Path":if case == "runtime-path" { "/" } else { "/setup/" },
                    "RoleName":boundary.name, "RoleId":if case == "replacement-id" { "other" } else { "owned-role-id" },
                    "Arn":"arn:aws:iam::123456789012:role/setup/_gateway-cleanup-example",
                    "CreateDate":"2026-01-01T00:00:00Z", "Tags":{"member":tags}
                })).unwrap();
                assert_eq!(boundary.verify_role(&role, restored.role_id.as_deref()).is_ok(), case == "owned", "{case}");
            }
            assert!(boundary.verify_tags(None).is_err());
        });
    }
}
