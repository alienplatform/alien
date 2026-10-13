//! One generation's exact private API invocation grant on a retained Lambda.

use alien_aws_clients::lambda::{AddPermissionRequest, LambdaApi, RemovePermissionRequest};
use alien_error::{AlienError, Context, IntoAlienError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::core::aws_tag_scoped::{is_remote_not_found, preserve_composite_cleanup_error};
use crate::{ErrorData, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GatewayPermission {
    function_arn: String,
    account: String,
    source_arn: String,
    statement_id: String,
    create_permitted: bool,
    retiring: bool,
}

impl GatewayPermission {
    pub fn new(function_arn: &str, account: &str, region: &str, api: &str) -> Result<Self> {
        if !function_arn.starts_with(&format!("arn:aws:lambda:{region}:{account}:function:"))
            || account.len() != 12
            || !account.bytes().all(|byte| byte.is_ascii_digit())
            || region.is_empty()
            || api.is_empty()
            || !api.bytes().all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err(invalid(
                "Private invocation grant needs the retained regional function and exact API",
            ));
        }
        Ok(Self {
            function_arn: function_arn.into(),
            account: account.into(),
            source_arn: format!("arn:aws:execute-api:{region}:{account}:{api}/private/*/*"),
            statement_id: format!("PrivateGateway-{}", Uuid::new_v4()),
            create_permitted: false,
            retiring: false,
        })
    }

    pub async fn step_prepare(&mut self, lambda: &dyn LambdaApi) -> Result<bool> {
        if self.retiring {
            return Err(invalid("Retiring invocation permission cannot be reused"));
        }
        if !self.create_permitted {
            self.create_permitted = true;
            return Ok(false);
        }
        let (present, revision) = self.read(lambda).await?;
        if present {
            return Ok(true);
        }
        lambda
            .add_permission(
                &self.function_arn,
                AddPermissionRequest::builder()
                    .statement_id(self.statement_id.clone())
                    .action("lambda:InvokeFunction".into())
                    .principal("apigateway.amazonaws.com".into())
                    .source_arn(self.source_arn.clone())
                    .source_account(self.account.clone())
                    .maybe_revision_id(revision)
                    .build(),
            )
            .await
            .context(cloud("Grant private API invocation"))?;
        // A subsequent policy read validates success, including a lost add reply.
        Ok(false)
    }

    pub async fn step_delete(&mut self, lambda: &dyn LambdaApi) -> Result<bool> {
        self.delete_inner(lambda)
            .await
            .map_err(preserve_composite_cleanup_error)
    }

    async fn delete_inner(&mut self, lambda: &dyn LambdaApi) -> Result<bool> {
        self.retiring = true;
        if !self.create_permitted {
            return Ok(true);
        }
        let (present, revision) = self.read(lambda).await?;
        if !present {
            return Ok(true);
        }
        lambda
            .remove_permission(
                &self.function_arn,
                RemovePermissionRequest::builder()
                    .statement_id(self.statement_id.clone())
                    .revision_id(
                        revision.ok_or_else(|| {
                            invalid("Lambda policy revision missing before removal")
                        })?,
                    )
                    .build(),
            )
            .await
            .context(cloud("Retire exact private API invocation grant"))?;
        Ok(false)
    }

    async fn read(&self, lambda: &dyn LambdaApi) -> Result<(bool, Option<String>)> {
        let response = match lambda.get_policy(&self.function_arn, None).await {
            Ok(response) => response,
            Err(error) if is_remote_not_found(&error) => return Ok((false, None)),
            Err(error) => {
                return Err(error.context(cloud("Read private API invocation permission")))
            }
        };
        let policy: Value = serde_json::from_str(
            response
                .policy
                .as_deref()
                .ok_or_else(|| invalid("Lambda policy contents missing"))?,
        )
        .into_alien_error()
        .context(cloud("Decode Lambda invocation policy"))?;
        let statements = policy
            .get("Statement")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid("Lambda policy statements missing"))?;
        let matches = statements
            .iter()
            .filter(|statement| {
                statement.get("Sid").and_then(Value::as_str) == Some(&self.statement_id)
            })
            .collect::<Vec<_>>();
        if matches.len() > 1
            || matches
                .first()
                .is_some_and(|statement| **statement != self.statement())
        {
            return Err(invalid(
                "Private invocation statement changed ownership or scope",
            ));
        }
        let revision = response
            .revision_id
            .filter(|revision| !revision.is_empty())
            .ok_or_else(|| invalid("Lambda policy revision missing"))?;
        Ok((!matches.is_empty(), Some(revision)))
    }

    fn statement(&self) -> Value {
        json!({
            "Sid":self.statement_id,"Effect":"Allow","Principal":{"Service":"apigateway.amazonaws.com"},
            "Action":"lambda:InvokeFunction","Resource":self.function_arn,
            "Condition":{"ArnLike":{"AWS:SourceArn":self.source_arn},"StringEquals":{"AWS:SourceAccount":self.account}}
        })
    }
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
    use alien_aws_clients::lambda::MockLambdaApi;
    use alien_client_core::ErrorData as ClientError;
    use std::sync::{Arc, Mutex};

    fn owner() -> GatewayPermission {
        GatewayPermission::new(
            "arn:aws:lambda:us-east-1:123456789012:function:worker",
            "123456789012",
            "us-east-1",
            "ownedapi",
        )
        .unwrap()
    }

    fn reload(owner: GatewayPermission) -> GatewayPermission {
        serde_json::from_value(serde_json::to_value(owner).unwrap()).unwrap()
    }

    #[tokio::test]
    async fn lost_add_and_delete_replies_recover_without_touching_other_grants() {
        let mut owner = owner();
        let expected = owner.statement();
        let statement_id = owner.statement_id.clone();
        let foreign = json!({"Sid":"public-retained","Effect":"Allow","Principal":{"Service":"s3.amazonaws.com"},"Action":"lambda:InvokeFunction","Resource":owner.function_arn});
        let policy = Arc::new(Mutex::new(vec![foreign.clone()]));
        let mut lambda = MockLambdaApi::new();
        let world = policy.clone();
        lambda.expect_get_policy().returning(move |function, qualifier| {
            assert_eq!(function,"arn:aws:lambda:us-east-1:123456789012:function:worker");
            assert!(qualifier.is_none());
            Ok(serde_json::from_value(json!({"Policy":json!({"Version":"2012-10-17","Statement":*world.lock().unwrap()}).to_string(),"RevisionId":"current"})).unwrap())
        });
        let world = policy.clone();
        lambda
            .expect_add_permission()
            .times(1)
            .returning(move |_, request| {
                assert_eq!(request.statement_id, statement_id);
                assert_eq!(request.principal, "apigateway.amazonaws.com");
                assert_eq!(
                    request.source_arn.as_deref(),
                    Some("arn:aws:execute-api:us-east-1:123456789012:ownedapi/private/*/*")
                );
                assert_eq!(request.source_account.as_deref(), Some("123456789012"));
                assert_eq!(request.revision_id.as_deref(), Some("current"));
                world.lock().unwrap().push(expected.clone());
                Err(AlienError::new(ClientError::Timeout {
                    message: "add committed; reply lost".into(),
                }))
            });
        let world = policy.clone();
        let sid = owner.statement_id.clone();
        lambda
            .expect_remove_permission()
            .times(1)
            .returning(move |_, request| {
                assert_eq!(request.statement_id, sid);
                assert_eq!(request.revision_id.as_deref(), Some("current"));
                world
                    .lock()
                    .unwrap()
                    .retain(|statement| statement["Sid"] != sid);
                Err(AlienError::new(ClientError::Timeout {
                    message: "remove committed; reply lost".into(),
                }))
            });
        assert!(!owner.step_prepare(&lambda).await.unwrap());
        owner = reload(owner);
        assert!(owner.step_prepare(&lambda).await.is_err());
        owner = reload(owner);
        assert!(owner.step_prepare(&lambda).await.unwrap());
        assert_eq!(policy.lock().unwrap().len(), 2);
        assert!(owner.step_delete(&lambda).await.is_err());
        owner = reload(owner);
        assert!(owner.step_delete(&lambda).await.unwrap());
        assert_eq!(*policy.lock().unwrap(), vec![foreign]);
        assert!(owner.step_prepare(&lambda).await.is_err());
    }

    #[tokio::test]
    async fn changed_scope_and_missing_revision_refuse_mutation() {
        for scenario in ["broader-source", "duplicate", "missing-revision"] {
            let mut owner = owner();
            owner.create_permitted = true;
            let mut statement = owner.statement();
            if scenario == "broader-source" {
                statement["Condition"]["ArnLike"]["AWS:SourceArn"] = json!("*");
            }
            let statements = if scenario == "duplicate" {
                vec![statement.clone(), statement]
            } else {
                vec![statement]
            };
            let mut lambda = MockLambdaApi::new();
            lambda.expect_get_policy().times(2).returning(move |_,_| {
                Ok(serde_json::from_value(json!({"Policy":json!({"Statement":statements}).to_string(),"RevisionId":if scenario == "missing-revision" {None} else {Some("current")}})).unwrap())
            });
            assert!(owner.step_prepare(&lambda).await.is_err(), "{scenario}");
            owner = reload(owner);
            assert!(owner.step_delete(&lambda).await.is_err(), "{scenario}");
            assert!(owner.create_permitted);
        }
    }

    #[tokio::test]
    async fn concurrent_policy_change_is_reread_before_removal_retry() {
        let mut owner = owner();
        owner.create_permitted = true;
        let expected = owner.statement();
        let version = Arc::new(Mutex::new(0));
        let mut lambda = MockLambdaApi::new();
        let world = version.clone();
        lambda.expect_get_policy().times(2).returning(move |_,_| {
            Ok(serde_json::from_value(json!({"Policy":json!({"Statement":[expected]}).to_string(),"RevisionId":world.lock().unwrap().to_string()})).unwrap())
        });
        let world = version.clone();
        lambda
            .expect_remove_permission()
            .times(2)
            .returning(move |_, request| {
                let mut version = world.lock().unwrap();
                assert_eq!(request.revision_id, Some(version.to_string()));
                *version += 1;
                if *version == 1 {
                    Err(AlienError::new(ClientError::HttpResponseError {
                        message: "revision conflict".into(),
                        url: "https://lambda.example.test".into(),
                        http_status: 412,
                        http_request_text: None,
                        http_response_text: Some("policy changed concurrently".into()),
                    }))
                } else {
                    Ok(())
                }
            });
        assert!(owner.step_delete(&lambda).await.is_err());
        owner = reload(owner);
        assert!(
            !owner.step_delete(&lambda).await.unwrap(),
            "successful delete still requires absence confirmation"
        );
        assert_eq!(*version.lock().unwrap(), 2);
    }
}
