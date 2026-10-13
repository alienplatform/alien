//! Generation-specific ownership for a private REST API. Routing is configured
//! only after the API and VPC endpoint identities have been checkpointed.

use std::collections::{BTreeSet, HashMap};

use alien_aws_clients::apigateway::{
    ApiGatewayApi, CreateDeploymentRequest, CreatePrivateRestApiRequest, CreateResourceRequest,
    PutIntegrationRequest, PutMethodRequest, RestApiDetails,
};
use alien_core::standard_resource_tags;
use alien_error::{AlienError, Context, ContextError};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::aws_gateway_config::PRIVATE_API_STAGE;
use crate::core::aws_tag_scoped::{
    is_remote_not_found, preserve_composite_cleanup_error, CREATE_ATTEMPT_TAG,
};
use crate::{ErrorData, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PrivateRestApi {
    name: String,
    tags: HashMap<String, String>,
    create_permitted: bool,
    create_attempted: bool,
    ids: BTreeSet<String>,
    delete_permitted: BTreeSet<String>,
    retiring: bool,
    routes: Option<RestRoutes>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct RestRoutePlan {
    pub api_id: String,
    pub root_id: String,
    pub region: String,
    pub function_arn: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RestRoutes {
    plan: RestRoutePlan,
    proxy_id: Option<String>,
    deployment_id: Option<String>,
    deployment_permitted: bool,
    deployment_attempted: bool,
}

impl PrivateRestApi {
    pub fn new(stack: &str, resource: &str) -> Result<Self> {
        let generation = Uuid::new_v4().to_string();
        let name = format!("{stack}-{resource}-private-{generation}");
        if stack.is_empty() || resource.is_empty() || name.len() > 128 {
            return Err(invalid(
                "Private REST API requires a valid stack/resource name",
            ));
        }
        let mut tags = standard_resource_tags(stack, resource);
        tags.insert(CREATE_ATTEMPT_TAG.into(), generation);
        Ok(Self {
            name,
            tags,
            create_permitted: false,
            create_attempted: false,
            ids: BTreeSet::new(),
            delete_permitted: BTreeSet::new(),
            retiring: false,
            routes: None,
        })
    }

    /// The parent must checkpoint every pending result and apply bounded polling.
    pub async fn step_create(
        &mut self,
        api: &dyn ApiGatewayApi,
    ) -> Result<Option<(String, String)>> {
        if self.retiring {
            return Err(invalid("Retiring private REST API cannot be reused"));
        }
        if !self.create_permitted {
            self.create_permitted = true;
            return Ok(None);
        }
        self.discover(api).await?;
        if self.ids.len() > 1 {
            return Err(invalid(
                "Multiple REST APIs committed for one generation; retained for cleanup",
            ));
        }
        if let Some(id) = self.ids.first() {
            let details = api
                .get_rest_api_details(id)
                .await
                .context(cloud("Read private REST API"))?;
            self.verify(&details)?;
            if details.id.as_deref() != Some(id) {
                return Err(invalid(
                    "Private REST API lookup returned a different identity",
                ));
            }
            let root = details
                .root_resource_id
                .ok_or_else(|| invalid("Private REST API root is missing"))?;
            return Ok(Some((id.clone(), root)));
        }
        if self.create_attempted {
            return Ok(None);
        }
        self.create_attempted = true;
        let created = api
            .create_private_rest_api(
                CreatePrivateRestApiRequest::builder()
                    .name(self.name.clone())
                    .binary_media_types(vec!["*/*".into()])
                    .vpc_endpoint_ids(vec![])
                    .policy(deny_policy())
                    // Native endpoint routing is required by the gateway. Its resource
                    // policy denies all callers until the exact VPC endpoint is ready.
                    .disable_execute_api_endpoint(false)
                    .tags(self.tags.clone())
                    .build(),
            )
            .await
            .context(cloud("Create private REST API"))?;
        let id = created
            .id
            .ok_or_else(|| invalid("Private REST API create returned no ID"))?;
        self.ids.insert(id);
        if created.name.as_deref() != Some(&self.name) {
            return Err(invalid("Private REST API create returned a different name"));
        }
        Ok(None)
    }

    /// Only this API's resource policy changes; the endpoint policy is owned by
    /// the separate endpoint receipt. Existing public API routing is untouched.
    pub async fn step_policy(
        &self,
        api: &dyn ApiGatewayApi,
        id: &str,
        endpoint_id: &str,
    ) -> Result<bool> {
        if self.retiring
            || self.ids.len() != 1
            || !self.ids.contains(id)
            || !endpoint_id.starts_with("vpce-")
            || !endpoint_id[5..].bytes().all(|b| b.is_ascii_hexdigit())
            || endpoint_id.len() <= 5
        {
            return Err(invalid(
                "Private REST API policy requires exact owned API and endpoint IDs",
            ));
        }
        let details = api
            .get_rest_api_details(id)
            .await
            .context(cloud("Read private REST invocation policy"))?;
        self.verify(&details)?;
        if details.id.as_deref() != Some(id) {
            return Err(invalid("Private REST policy lookup identity changed"));
        }
        let policy = serde_json::json!({"Version":"2012-10-17","Statement":[{
            "Effect":"Allow","Principal":"*","Action":"execute-api:Invoke","Resource":"execute-api:/*",
            "Condition":{"StringEquals":{"aws:SourceVpce":endpoint_id}}
        }]}).to_string();
        if policies_equal(details.policy.as_deref(), &policy) {
            return Ok(true);
        }
        api.update_rest_api_policy(id, &policy)
            .await
            .context(cloud("Restrict private REST invocation to owned endpoint"))?;
        Ok(false)
    }

    /// Bind one immutable Lambda integration to this API generation. A function
    /// code/config update retains this ARN and therefore does not redeploy routes.
    pub fn configure_routes(&mut self, plan: RestRoutePlan) -> Result<()> {
        if self.retiring
            || self.ids.len() != 1
            || !self.ids.contains(&plan.api_id)
            || plan.root_id.is_empty()
            || plan.region.is_empty()
            || !plan
                .function_arn
                .starts_with(&format!("arn:aws:lambda:{}:", plan.region))
            || !plan.function_arn.contains(":function:")
        {
            return Err(invalid(
                "Private REST routing requires the owned API/root and a regional Lambda ARN",
            ));
        }
        if let Some(routes) = &self.routes {
            if routes.plan != plan {
                return Err(invalid("Private REST integration identity changed"));
            }
        } else {
            self.routes = Some(RestRoutes {
                plan,
                proxy_id: None,
                deployment_id: None,
                deployment_permitted: false,
                deployment_attempted: false,
            });
        }
        Ok(())
    }

    /// Advance exactly one routing mutation, then let the parent checkpoint.
    pub async fn step_routes(&mut self, api: &dyn ApiGatewayApi) -> Result<bool> {
        if self.retiring {
            return Err(invalid("Retiring private REST routes cannot be reused"));
        }
        let plan = self
            .routes
            .as_ref()
            .ok_or_else(|| invalid("Private REST route plan is missing"))?
            .plan
            .clone();
        let details = api
            .get_rest_api_details(&plan.api_id)
            .await
            .context(cloud("Read private REST routing owner"))?;
        self.verify(&details)?;
        if details.id.as_deref() != Some(&plan.api_id)
            || details.root_resource_id.as_deref() != Some(&plan.root_id)
        {
            return Err(invalid("Private REST routing owner/root changed"));
        }
        let mut position = None;
        let mut cursors = BTreeSet::new();
        let mut proxy = None;
        loop {
            let page = api
                .get_resources(&plan.api_id, position)
                .await
                .context(cloud("Recover private REST proxy resource"))?;
            for resource in page.items {
                if resource.path.as_deref() != Some("/{proxy+}") {
                    continue;
                }
                if proxy.is_some()
                    || resource.parent_id.as_deref() != Some(&plan.root_id)
                    || resource.path_part.as_deref() != Some("{proxy+}")
                {
                    return Err(invalid(
                        "Private REST proxy resource is ambiguous or has another parent",
                    ));
                }
                proxy = Some(
                    resource
                        .id
                        .ok_or_else(|| invalid("Private REST proxy resource has no ID"))?,
                );
            }
            position = page.position;
            match &position {
                None => break,
                Some(cursor) if cursors.insert(cursor.clone()) => {}
                Some(_) => {
                    return Err(invalid("Private REST resource discovery repeated a cursor"))
                }
            }
        }
        let routes = self
            .routes
            .as_mut()
            .ok_or_else(|| invalid("Private REST route plan disappeared"))?;
        if let Some(proxy) = proxy {
            if routes.proxy_id.as_ref().is_some_and(|id| id != &proxy) {
                return Err(invalid("Private REST proxy resource identity changed"));
            }
            routes.proxy_id = Some(proxy);
        } else {
            if routes.proxy_id.is_some() {
                return Ok(false);
            }
            let created = api
                .create_resource(
                    &plan.api_id,
                    &plan.root_id,
                    CreateResourceRequest {
                        path_part: "{proxy+}".into(),
                    },
                )
                .await
                .context(cloud("Create private REST proxy resource"))?;
            routes.proxy_id = Some(
                created
                    .id
                    .ok_or_else(|| invalid("Private REST proxy create returned no ID"))?,
            );
            return Ok(false);
        }
        let proxy_id = routes
            .proxy_id
            .as_ref()
            .ok_or_else(|| invalid("Private REST proxy ID is missing"))?;
        let uri = format!("arn:aws:apigateway:{}:lambda:path/2021-11-15/functions/{}/response-streaming-invocations", plan.region, plan.function_arn);
        for resource_id in [&plan.root_id, proxy_id] {
            let method = match api.get_method(&plan.api_id, resource_id, "ANY").await {
                Ok(method) => method,
                Err(error) if is_remote_not_found(&error) => {
                    api.put_method(
                        &plan.api_id,
                        resource_id,
                        "ANY",
                        PutMethodRequest {
                            authorization_type: "NONE".into(),
                        },
                    )
                    .await
                    .context(cloud("Create private REST catch-all method"))?;
                    return Ok(false);
                }
                Err(error) => {
                    return Err(error.context(cloud("Read private REST catch-all method")))
                }
            };
            if method.http_method.as_deref() != Some("ANY")
                || method.authorization_type.as_deref() != Some("NONE")
                || method.api_key_required == Some(true)
            {
                return Err(invalid("Private REST method authentication changed"));
            }
            if method
                .method_integration
                .as_ref()
                .is_none_or(|integration| {
                    integration.integration_type != "AWS_PROXY"
                        || integration.integration_http_method != "POST"
                        || integration.uri != uri
                        || integration.response_transfer_mode.as_deref() != Some("STREAM")
                        || integration.timeout_in_millis != Some(900_000)
                })
            {
                if routes.deployment_permitted {
                    return Err(invalid("Deployed private REST integration changed"));
                }
                api.put_integration(
                    &plan.api_id,
                    resource_id,
                    "ANY",
                    PutIntegrationRequest::builder()
                        .integration_type("AWS_PROXY".into())
                        .integration_http_method("POST".into())
                        .uri(uri.clone())
                        .response_transfer_mode("STREAM".into())
                        .timeout_in_millis(900_000)
                        .build(),
                )
                .await
                .context(cloud("Configure private REST streaming integration"))?;
                return Ok(false);
            }
        }
        let stage = match api.get_stage(&plan.api_id, PRIVATE_API_STAGE).await {
            Ok(stage) => Some(stage),
            Err(error) if is_remote_not_found(&error) => None,
            Err(error) => return Err(error.context(cloud("Recover private REST deployment stage"))),
        };
        if let Some(stage) = stage {
            if !routes.deployment_permitted
                || stage.stage_name.as_deref() != Some(PRIVATE_API_STAGE)
            {
                return Err(invalid(
                    "Private REST stage exists without recorded deployment intent",
                ));
            }
            let deployment = stage
                .deployment_id
                .ok_or_else(|| invalid("Private REST stage has no deployment ID"))?;
            if routes
                .deployment_id
                .as_ref()
                .is_some_and(|id| id != &deployment)
            {
                return Err(invalid("Private REST stage deployment changed"));
            }
            routes.deployment_id = Some(deployment);
            if stage.tags.as_ref().is_some_and(|tags| {
                self.tags
                    .iter()
                    .all(|(key, value)| tags.get(key) == Some(value))
            }) {
                return Ok(true);
            }
            if stage.tags.as_ref().is_some_and(|tags| {
                self.tags
                    .iter()
                    .any(|(key, value)| tags.get(key).is_some_and(|actual| actual != value))
            }) {
                return Err(invalid("Private REST stage carries foreign ownership tags"));
            }
            api.tag_resource(
                &format!(
                    "arn:aws:apigateway:{}::/restapis/{}/stages/{PRIVATE_API_STAGE}",
                    plan.region, plan.api_id
                ),
                self.tags.clone(),
            )
            .await
            .context(cloud("Tag owned private REST stage"))?;
            return Ok(false);
        }
        if routes.deployment_id.is_some() || routes.deployment_attempted {
            return Ok(false);
        }
        if !routes.deployment_permitted {
            routes.deployment_permitted = true;
            return Ok(false);
        }
        routes.deployment_attempted = true;
        let deployment = api
            .create_deployment(
                &plan.api_id,
                CreateDeploymentRequest {
                    stage_name: PRIVATE_API_STAGE.into(),
                },
            )
            .await
            .context(cloud("Deploy private REST streaming routes"))?;
        routes.deployment_id = Some(
            deployment
                .id
                .ok_or_else(|| invalid("Private REST deployment create returned no ID"))?,
        );
        Ok(false)
    }

    pub async fn step_delete(&mut self, api: &dyn ApiGatewayApi) -> Result<bool> {
        self.delete_inner(api)
            .await
            .map_err(preserve_composite_cleanup_error)
    }

    async fn delete_inner(&mut self, api: &dyn ApiGatewayApi) -> Result<bool> {
        self.retiring = true;
        if !self.create_permitted {
            return Ok(true);
        }
        self.discover(api).await?;
        if self.ids.is_empty() {
            // Empty discovery cannot disprove a committed request whose reply was lost.
            return Ok(false);
        }
        for id in &self.ids {
            let details = match api.get_rest_api_details(id).await {
                Ok(details) => details,
                Err(error) if is_remote_not_found(&error) => {
                    if self.delete_permitted.contains(id) {
                        continue;
                    }
                    return Err(invalid(
                        "Private REST API disappeared before owned deletion",
                    ));
                }
                Err(error) => {
                    return Err(error.context(cloud("Read private REST API before cleanup")))
                }
            };
            self.verify(&details)?;
            if details.id.as_deref() != Some(id) {
                return Err(invalid("Private REST cleanup identity changed"));
            }
            if self.delete_permitted.insert(id.clone()) {
                return Ok(false);
            }
            match api.delete_rest_api(id).await {
                Ok(()) => {}
                Err(error) if is_remote_not_found(&error) => {}
                Err(error) => return Err(error.context(cloud("Delete owned private REST API"))),
            }
            return Ok(false);
        }
        Ok(true)
    }

    async fn discover(&mut self, api: &dyn ApiGatewayApi) -> Result<()> {
        let mut position = None;
        let mut cursors = BTreeSet::new();
        loop {
            let page = api
                .get_rest_apis(position)
                .await
                .context(cloud("Discover private REST API generation"))?;
            for details in page.items {
                if details.name.as_deref() != Some(&self.name) {
                    continue;
                }
                self.verify(&details)?;
                let id = details
                    .id
                    .ok_or_else(|| invalid("Discovered private REST API has no ID"))?;
                self.ids.insert(id);
            }
            position = page.position;
            match &position {
                None => return Ok(()),
                Some(cursor) if cursors.insert(cursor.clone()) => {}
                Some(_) => {
                    return Err(invalid(
                        "Private REST discovery repeated its pagination cursor",
                    ))
                }
            }
        }
    }

    fn verify(&self, details: &RestApiDetails) -> Result<()> {
        let tags = details
            .tags
            .as_ref()
            .ok_or_else(|| invalid("Private REST ownership tags missing"))?;
        if details.name.as_deref() != Some(&self.name)
            || !self
                .tags
                .iter()
                .all(|(key, value)| tags.get(key) == Some(value))
            || details
                .endpoint_configuration
                .as_ref()
                .is_none_or(|config| config.types != ["PRIVATE"])
            || details.disable_execute_api_endpoint != Some(false)
            || details
                .binary_media_types
                .as_ref()
                .is_none_or(|types| types != &["*/*"])
        {
            return Err(invalid(
                "Private REST ownership or endpoint configuration changed",
            ));
        }
        Ok(())
    }
}

fn deny_policy() -> String {
    serde_json::json!({"Version":"2012-10-17","Statement":[{"Effect":"Deny","Principal":"*","Action":"execute-api:Invoke","Resource":"*"}]}).to_string()
}

fn policies_equal(actual: Option<&str>, expected: &str) -> bool {
    actual.and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
        == serde_json::from_str::<serde_json::Value>(expected).ok()
}

fn invalid(message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        message: message.into(),
        resource_id: None,
    })
}
fn cloud(message: &str) -> ErrorData {
    ErrorData::CloudPlatformError {
        message: message.into(),
        resource_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_aws_clients::apigateway::{MockApiGatewayApi, RestApi, RestApisPage};
    use alien_client_core::ErrorData as ClientError;
    use std::sync::{Arc, Mutex};

    fn details(owner: &PrivateRestApi, id: &str) -> RestApiDetails {
        serde_json::from_value(
            serde_json::json!({"id":id,"name":owner.name,"rootResourceId":"root",
            "tags":owner.tags,"endpointConfiguration":{"types":["PRIVATE"]},
            "disableExecuteApiEndpoint":false,"binaryMediaTypes":["*/*"],"policy":deny_policy()}),
        )
        .unwrap()
    }
    fn reload(owner: &PrivateRestApi) -> PrivateRestApi {
        serde_json::from_value(serde_json::to_value(owner).unwrap()).unwrap()
    }

    #[tokio::test]
    async fn lost_create_and_delete_replies_recover_exact_generation_after_reload() {
        let mut owner = PrivateRestApi::new("stack", "worker").unwrap();
        let remote = Arc::new(Mutex::new(None::<RestApiDetails>));
        let mut api = MockApiGatewayApi::new();
        let world = remote.clone();
        api.expect_get_rest_apis().returning(move |_| {
            Ok(RestApisPage {
                items: world.lock().unwrap().clone().into_iter().collect(),
                position: None,
            })
        });
        let world = remote.clone();
        let created = details(&owner, "owned");
        api.expect_create_private_rest_api()
            .times(1)
            .returning(move |request| {
                assert_eq!(request.binary_media_types, Some(vec!["*/*".into()]));
                assert!(policies_equal(Some(&request.policy), &deny_policy()));
                assert!(!request.disable_execute_api_endpoint);
                *world.lock().unwrap() = Some(created.clone());
                Err(AlienError::new(ClientError::InvalidInput {
                    message: "lost create reply".into(),
                    field_name: None,
                }))
            });
        let world = remote.clone();
        api.expect_get_rest_api_details().returning(move |id| {
            assert_eq!(id, "owned");
            world.lock().unwrap().clone().ok_or_else(|| {
                AlienError::new(ClientError::RemoteResourceNotFound {
                    resource_type: "REST API".into(),
                    resource_name: id.into(),
                })
            })
        });
        let world = remote.clone();
        api.expect_delete_rest_api().times(1).returning(move |id| {
            assert_eq!(id, "owned");
            *world.lock().unwrap() = None;
            Err(AlienError::new(ClientError::InvalidInput {
                message: "lost delete reply".into(),
                field_name: None,
            }))
        });
        assert!(owner.step_create(&api).await.unwrap().is_none());
        owner = reload(&owner);
        assert!(owner.step_create(&api).await.is_err());
        owner = reload(&owner);
        assert_eq!(
            owner.step_create(&api).await.unwrap(),
            Some(("owned".into(), "root".into()))
        );
        assert!(!owner.step_delete(&api).await.unwrap());
        owner = reload(&owner);
        assert!(owner.step_delete(&api).await.is_err());
        owner = reload(&owner);
        assert!(owner.step_delete(&api).await.unwrap());
    }

    #[tokio::test]
    async fn foreign_generation_is_not_adopted_or_deleted_and_empty_uncertainty_is_retained() {
        let mut owner = PrivateRestApi::new("stack", "worker").unwrap();
        let mut foreign = details(&owner, "foreign");
        foreign
            .tags
            .as_mut()
            .unwrap()
            .insert(CREATE_ATTEMPT_TAG.into(), "foreign".into());
        let mut api = MockApiGatewayApi::new();
        api.expect_get_rest_apis().returning(move |_| {
            Ok(RestApisPage {
                items: vec![foreign.clone()],
                position: None,
            })
        });
        assert!(owner.step_create(&api).await.unwrap().is_none());
        assert!(owner.step_create(&api).await.is_err());
        assert!(owner.step_delete(&api).await.is_err());
        assert!(owner.ids.is_empty());
        let mut api = MockApiGatewayApi::new();
        api.expect_get_rest_apis().returning(|_| {
            Ok(RestApisPage {
                items: vec![],
                position: None,
            })
        });
        assert!(!reload(&owner).step_delete(&api).await.unwrap());
    }

    #[tokio::test]
    async fn successful_create_checkpoints_id_before_optional_response_validation() {
        let mut owner = PrivateRestApi::new("stack", "worker").unwrap();
        let mut api = MockApiGatewayApi::new();
        api.expect_get_rest_apis().returning(|_| {
            Ok(RestApisPage {
                items: vec![],
                position: None,
            })
        });
        api.expect_create_private_rest_api()
            .times(1)
            .returning(|_| {
                Ok(RestApi {
                    id: Some("owned".into()),
                    name: None,
                    root_resource_id: None,
                })
            });
        assert!(owner.step_create(&api).await.unwrap().is_none());
        assert!(owner.step_create(&api).await.is_err());
        assert_eq!(reload(&owner).ids, BTreeSet::from(["owned".into()]));
    }
    #[tokio::test]
    async fn routing_recovers_every_lost_mutation_reply_without_duplicate_deployment() {
        use alien_aws_clients::apigateway::{ResourcesPage, RestMethod, RestStage};
        #[derive(Default)]
        struct World {
            proxy: bool,
            methods: HashMap<String, RestMethod>,
            stage: Option<RestStage>,
        }
        fn lost() -> AlienError<ClientError> {
            AlienError::new(ClientError::InvalidInput {
                message: "committed reply lost".into(),
                field_name: None,
            })
        }
        fn missing() -> AlienError<ClientError> {
            AlienError::new(ClientError::RemoteResourceNotFound {
                resource_type: "REST child".into(),
                resource_name: "missing".into(),
            })
        }
        let world = Arc::new(Mutex::new(World::default()));
        let mut owner = PrivateRestApi::new("stack", "worker").unwrap();
        owner.ids.insert("owned".into());
        owner.create_permitted = true;
        owner
            .configure_routes(RestRoutePlan {
                api_id: "owned".into(),
                root_id: "root".into(),
                region: "us-east-1".into(),
                function_arn: "arn:aws:lambda:us-east-1:123456789012:function:worker".into(),
            })
            .unwrap();
        let mut api = MockApiGatewayApi::new();
        let detail = details(&owner, "owned");
        api.expect_get_rest_api_details().returning(move |id| {
            assert_eq!(id, "owned");
            Ok(detail.clone())
        });
        let remote = world.clone();
        api.expect_get_resources().returning(move |id,position| {
            assert_eq!(id,"owned"); assert!(position.is_none());
            Ok(ResourcesPage { items: if remote.lock().unwrap().proxy {
                vec![serde_json::from_value(serde_json::json!({"id":"proxy","parentId":"root","pathPart":"{proxy+}","path":"/{proxy+}"})).unwrap()]
            } else { vec![] }, position: None })
        });
        let remote = world.clone();
        api.expect_create_resource()
            .times(1)
            .returning(move |id, parent, request| {
                assert_eq!(id, "owned");
                assert_eq!(parent, "root");
                assert_eq!(request.path_part, "{proxy+}");
                remote.lock().unwrap().proxy = true;
                Err(lost())
            });
        let remote = world.clone();
        api.expect_get_method()
            .returning(move |id, resource, method| {
                assert_eq!(id, "owned");
                assert_eq!(method, "ANY");
                remote
                    .lock()
                    .unwrap()
                    .methods
                    .get(resource)
                    .cloned()
                    .ok_or_else(missing)
            });
        let remote = world.clone();
        api.expect_put_method()
            .times(2)
            .returning(move |id, resource, method, request| {
                assert_eq!(id, "owned");
                assert_eq!(method, "ANY");
                assert_eq!(request.authorization_type, "NONE");
                assert!(remote
                    .lock()
                    .unwrap()
                    .methods
                    .insert(
                        resource.into(),
                        RestMethod {
                            http_method: Some("ANY".into()),
                            authorization_type: Some("NONE".into()),
                            api_key_required: Some(false),
                            method_integration: None
                        }
                    )
                    .is_none());
                Err(lost())
            });
        let remote = world.clone();
        api.expect_put_integration()
            .times(2)
            .returning(move |id, resource, method, request| {
                assert_eq!(id, "owned");
                assert_eq!(method, "ANY");
                assert_eq!(request.response_transfer_mode.as_deref(), Some("STREAM"));
                assert!(remote
                    .lock()
                    .unwrap()
                    .methods
                    .get_mut(resource)
                    .unwrap()
                    .method_integration
                    .replace(request)
                    .is_none());
                Err(lost())
            });
        let remote = world.clone();
        api.expect_get_stage().returning(move |id, name| {
            assert_eq!(id, "owned");
            assert_eq!(name, PRIVATE_API_STAGE);
            remote.lock().unwrap().stage.clone().ok_or_else(missing)
        });
        let remote = world.clone();
        api.expect_create_deployment()
            .times(1)
            .returning(move |id, request| {
                assert_eq!(id, "owned");
                assert_eq!(request.stage_name, PRIVATE_API_STAGE);
                remote.lock().unwrap().stage = Some(RestStage {
                    stage_name: Some(PRIVATE_API_STAGE.into()),
                    deployment_id: Some("deployment".into()),
                    tags: None,
                });
                Err(lost())
            });
        let remote = world.clone();
        api.expect_tag_resource()
            .times(1)
            .returning(move |arn, tags| {
                assert_eq!(
                    arn,
                    "arn:aws:apigateway:us-east-1::/restapis/owned/stages/private"
                );
                remote.lock().unwrap().stage.as_mut().unwrap().tags = Some(tags);
                Err(lost())
            });
        let mut ready = false;
        let mut failures = 0;
        for _ in 0..16 {
            match owner.step_routes(&api).await {
                Ok(true) => {
                    ready = true;
                    break;
                }
                Ok(false) => {}
                Err(error) => {
                    assert!(format!("{error:?}").contains("committed reply lost"));
                    failures += 1;
                }
            }
            owner = reload(&owner);
        }
        assert!(ready);
        assert_eq!(failures, 7);
        assert_eq!(
            owner.routes.as_ref().unwrap().deployment_id.as_deref(),
            Some("deployment")
        );
        assert!(reload(&owner).step_routes(&api).await.unwrap());
    }
}
