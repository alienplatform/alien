//! A Live ECS cluster for one gateway generation. Its shared service-linked role
//! must already exist from setup; runtime never creates or deletes that role.

use std::collections::BTreeMap;

use alien_aws_clients::{
    ecs::{Cluster, ClusterRequest, CreateClusterRequest, DescribeClustersRequest, EcsApi, Tag},
    iam::IamApi,
};
use alien_core::standard_resource_tags;
use alien_error::{AlienError, Context, ContextError};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::aws_tag_scoped::{
    is_remote_access_denied, is_remote_not_found, preserve_composite_cleanup_error,
    CREATE_ATTEMPT_TAG,
};
use crate::{ErrorData, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GatewayCluster {
    name: String,
    arn: String,
    service_role_arn: String,
    tags: Vec<Tag>,
    create_permitted: bool,
    observed: bool,
    retiring: bool,
    delete_permitted: bool,
}

impl GatewayCluster {
    pub fn new(account: &str, region: &str, stack: &str, resource: &str) -> Result<Self> {
        let generation = Uuid::new_v4().to_string();
        let name = format!("{stack}-{resource}-gateway-{generation}");
        if account.len() != 12
            || !account.bytes().all(|byte| byte.is_ascii_digit())
            || region.is_empty()
            || stack.is_empty()
            || resource.is_empty()
            || name.len() > 255
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(invalid(
                "Gateway cluster requires valid account, region and generation name",
            ));
        }
        let mut tags = standard_resource_tags(stack, resource);
        tags.insert(CREATE_ATTEMPT_TAG.into(), generation);
        Ok(Self { arn:format!("arn:aws:ecs:{region}:{account}:cluster/{name}"), name,
            service_role_arn:format!("arn:aws:iam::{account}:role/aws-service-role/ecs.amazonaws.com/AWSServiceRoleForECS"),
            tags:tags.into_iter().map(|(key,value)|Tag{key,value}).collect(),
            create_permitted:false,observed:false,retiring:false,delete_permitted:false })
    }

    /// The parent must run this together with all other setup checks before
    /// creating any child of the private ingress graph.
    pub async fn validate_setup(&self, iam: &dyn IamApi) -> Result<()> {
        let role = match iam.get_role("AWSServiceRoleForECS").await {
            Ok(response) => response.get_role_result.role,
            Err(error) if is_remote_not_found(&error) || is_remote_access_denied(&error) => {
                return Err(invalid("Private worker gateway requires setup to provision the ECS service-linked role and grant its metadata read"));
            }
            Err(error) => {
                return Err(error.context(cloud("Read setup-owned ECS service-linked role")))
            }
        };
        if role.arn != self.service_role_arn || role.path != "/aws-service-role/ecs.amazonaws.com/"
        {
            return Err(invalid(
                "ECS service-linked role does not match the setup account and path",
            ));
        }
        Ok(())
    }

    pub async fn step_create(
        &mut self,
        ecs: &dyn EcsApi,
        iam: &dyn IamApi,
    ) -> Result<Option<String>> {
        if self.retiring {
            return Err(invalid("A retiring gateway cluster cannot be reused"));
        }
        self.validate_setup(iam).await?;
        if !self.create_permitted {
            self.create_permitted = true;
            return Ok(None);
        }
        match self.find(ecs).await? {
            Some(cluster) => {
                self.observed = true;
                match cluster.status.as_deref() {
                    Some("ACTIVE") => Ok(Some(self.arn.clone())),
                    Some("PROVISIONING") => Ok(None),
                    _ => Err(invalid("Gateway cluster failed to become active")),
                }
            }
            None => {
                if self.observed {
                    return Err(invalid(
                        "Observed gateway cluster disappeared; refusing replacement",
                    ));
                }
                self.create(ecs).await?;
                Ok(None)
            }
        }
    }

    pub async fn step_delete(&mut self, ecs: &dyn EcsApi, iam: &dyn IamApi) -> Result<bool> {
        self.delete_inner(ecs, iam)
            .await
            .map_err(preserve_composite_cleanup_error)
    }

    async fn delete_inner(&mut self, ecs: &dyn EcsApi, iam: &dyn IamApi) -> Result<bool> {
        self.retiring = true;
        if !self.create_permitted {
            return Ok(true);
        }
        let Some(cluster) = self.find(ecs).await? else {
            if self.delete_permitted {
                return Ok(true);
            }
            if self.observed {
                return Err(invalid("Gateway cluster disappeared before deletion"));
            }
            self.validate_setup(iam).await?;
            self.create(ecs).await?;
            return Ok(false);
        };
        self.observed = true;
        if cluster.status.as_deref() == Some("INACTIVE") {
            if !self.delete_permitted {
                return Err(invalid("Gateway cluster became inactive before deletion"));
            }
            return Ok(true);
        }
        if cluster.active_services_count != Some(0)
            || cluster.running_tasks_count != Some(0)
            || cluster.pending_tasks_count != Some(0)
        {
            return Ok(false);
        }
        if !self.delete_permitted {
            self.delete_permitted = true;
            return Ok(false);
        }
        ecs.delete_cluster(ClusterRequest {
            cluster: self.arn.clone(),
        })
        .await
        .context(cloud("Delete empty gateway cluster"))?;
        Ok(false)
    }

    async fn create(&self, ecs: &dyn EcsApi) -> Result<()> {
        let response = ecs
            .create_cluster(CreateClusterRequest {
                cluster_name: self.name.clone(),
                tags: self.tags.clone(),
            })
            .await
            .context(cloud("Create owned gateway cluster"))?;
        self.verify(&response.cluster)
    }

    async fn find(&self, ecs: &dyn EcsApi) -> Result<Option<Cluster>> {
        let response = ecs
            .describe_clusters(DescribeClustersRequest {
                clusters: vec![self.arn.clone()],
                include: Some(vec!["TAGS".into()]),
            })
            .await
            .context(cloud("Read gateway cluster ownership"))?;
        if !response.failures.is_empty() {
            if response.clusters.is_empty() && response.failures.len() == 1 {
                let failure = &response.failures[0];
                if failure.arn.as_deref() == Some(self.arn.as_str())
                    && failure.reason.as_deref() == Some("MISSING")
                {
                    return Ok(None);
                }
            }
            return Err(invalid(
                "Gateway cluster lookup reported unexpected batch failures",
            ));
        }
        if response.clusters.len() != 1 {
            return Err(invalid(
                "Gateway cluster lookup did not return exactly one cluster",
            ));
        }
        let cluster = response
            .clusters
            .into_iter()
            .next()
            .ok_or_else(|| invalid("Gateway cluster lookup is empty"))?;
        self.verify(&cluster)?;
        Ok(Some(cluster))
    }

    fn verify(&self, cluster: &Cluster) -> Result<()> {
        let tags: BTreeMap<_, _> = cluster
            .tags
            .iter()
            .map(|tag| (&tag.key, &tag.value))
            .collect();
        if cluster.cluster_arn != self.arn
            || cluster.cluster_name != self.name
            || tags.len() != cluster.tags.len()
            || self
                .tags
                .iter()
                .any(|tag| tags.get(&tag.key) != Some(&&tag.value))
        {
            return Err(invalid(
                "Gateway cluster identity or generation tags changed",
            ));
        }
        Ok(())
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
