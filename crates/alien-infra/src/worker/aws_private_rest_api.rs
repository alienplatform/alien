//! Generation-specific ownership for a private REST API. Routing is configured
//! only after the API and VPC endpoint identities have been checkpointed.

use std::collections::{BTreeSet, HashMap};

use alien_aws_clients::apigateway::{ApiGatewayApi, CreatePrivateRestApiRequest, RestApiDetails};
use alien_core::standard_resource_tags;
use alien_error::{AlienError, Context, ContextError};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
}
