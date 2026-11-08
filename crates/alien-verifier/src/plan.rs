//! The verifier's request, where it runs, and what an accepted release signs for a function:
//! who may invoke it and the worker the signed stack declares.
//!
//! Nothing here touches the network.

use std::collections::{BTreeMap, BTreeSet};

use alien_core::{aws_naming::aws_iam_role_name, gcp_naming::gcp_service_account_id};
use alien_error::{AlienError, Context};
use alien_signing::release::WorkloadKind;
use alien_signing::{BindingContext, InstallId, ReleaseBody, Workload};
use serde::{Deserialize, Serialize};

use crate::error::{ErrorData, Result};

/// Environment variable with the secrets a protected function loads at start: the signed
/// workload's list. Unprotected functions keep `ALIEN_SECRETS`, which the deployment writes; the
/// verifier refuses that one here.
pub const SECRETS_ENV: &str = "WORKLOAD_SECRETS";

/// A call to the verifier. Unknown fields are refused: the control plane
/// passes signed statements and the values the release names as provided,
/// never a function spec.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum Request {
    /// Check a cloud write a controller built for one signed workload, then send it unchanged.
    ApplyWrite(crate::cloud_write::ApplyWrite),
    /// Roll a machine cluster to the release's launch template (AWS).
    RollMachines(crate::machines::RollMachines),
    /// Resize a machine cluster within its signed bounds (GCP, Azure).
    Scale(crate::machines::Scale),
    /// Delete named machines of a group, never below its signed minimum
    /// (GCP, Azure).
    RemoveMachines(crate::machines::RemoveMachines),
    /// Delete a Live workload's function, service or app (uninstall).
    DeleteWorkload(DeleteWorkload),
    /// Delete a machine group with what it boots from (uninstall), one step per call.
    DeleteMachines(crate::machines::DeleteMachines),
}

/// Delete the function, Cloud Run service or Container App of a Live workload, with everything
/// the cloud attaches to it. No statement is needed: a delete can only cause an outage, which
/// a control plane can already cause by not deploying.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeleteWorkload {
    /// The workload, whose cloud resource is `<prefix>-<workload>`.
    pub workload: String,
}

/// Where the verifier runs, as it learned from its own identity: what it
/// names functions and identities, and the binding context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// The install.
    pub install: InstallId,
    /// The install's resource prefix.
    pub prefix: String,
    /// The network setup puts functions in, if any. Setup fixes it in the
    /// verifier's own configuration, which management can't change.
    pub vpc: Option<SetupNetwork>,
    /// The subnets and security groups setup created for machines, if the
    /// install runs any. Fixed the same way as `vpc`.
    pub machine_network: Option<SetupNetwork>,
}

/// Subnets and security groups setup created, fixed in the verifier's own
/// configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetupNetwork {
    /// Subnet ids.
    pub subnet_ids: Vec<String>,
    /// Security group ids (AWS), network tags (GCP) or network security
    /// group ids (Azure).
    pub security_group_ids: Vec<String>,
    /// What machines run as, where the cloud takes it at launch: the GCP
    /// service account email, or the Azure user-assigned identity id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    /// Which traffic a Cloud Run function sends through the subnet: `ALL_TRAFFIC`, or
    /// `PRIVATE_RANGES_ONLY` on the project's default network. GCP functions only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub egress: Option<String>,
}

impl Place {
    /// The platform name releases use.
    pub fn platform(&self) -> &'static str {
        match self.install {
            InstallId::Aws { .. } => "aws",
            InstallId::Gcp { .. } => "gcp",
            InstallId::Azure { .. } => "azure",
            InstallId::Kubernetes { .. } => "kubernetes",
            InstallId::Machine { .. } => "machines",
        }
    }

    /// The binding context for `binding_env`.
    pub fn binding_context(&self) -> BindingContext {
        let (account, region) = match &self.install {
            InstallId::Aws {
                account, region, ..
            } => (account.clone(), Some(region.clone())),
            InstallId::Gcp { project, .. } => (project.clone(), None),
            InstallId::Azure { subscription, .. } => (subscription.clone(), None),
            InstallId::Kubernetes { cluster, .. } => (cluster.clone(), None),
            InstallId::Machine { id } => (id.clone(), None),
        };
        BindingContext {
            platform: self.platform().to_string(),
            prefix: self.prefix.clone(),
            account,
            region,
        }
    }

    /// The name setup gives the function of `workload`: `<prefix>-<workload>`.
    /// A name the cloud would need shortened is refused, so the verifier
    /// never guesses which resource is meant.
    pub fn function_name(&self, workload: &str) -> Result<String> {
        let name = format!("{}-{workload}", self.prefix);
        let max = match self.install {
            InstallId::Aws { .. } => 64,
            InstallId::Gcp { .. } => 49,
            _ => 32,
        };
        let plain = name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            && !name.ends_with('-');
        if name.len() > max || !plain {
            return Err(refused(format!(
                "function name '{name}' is not one setup creates as is"
            )));
        }
        Ok(name)
    }

    /// What a function with permission profile `profile` runs as: the role
    /// setup creates for the profile's service account.
    pub fn run_as(&self, profile: &str) -> String {
        let name = format!("{}-{profile}-sa", self.prefix);
        match &self.install {
            InstallId::Aws { account, .. } => format!(
                "arn:aws:iam::{account}:role/{}",
                aws_iam_role_name(&self.prefix, &format!("{profile}-sa"))
            ),
            InstallId::Gcp { project, .. } => format!(
                "{}@{project}.iam.gserviceaccount.com",
                gcp_service_account_id(&self.prefix, &format!("{profile}-sa"))
            ),
            InstallId::Azure {
                subscription,
                resource_group,
            } => format!(
                "/subscriptions/{subscription}/resourceGroups/{resource_group}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{name}"
            ),
            _ => name,
        }
    }

    /// The architecture of a function that names none: what the engine
    /// creates on this platform.
    pub(crate) fn default_architecture(&self) -> &'static str {
        match self.install {
            InstallId::Aws { .. } => "arm64",
            _ => "x86_64",
        }
    }
}

/// One statement of a Lambda function's resource policy: a service that may invoke it, for one
/// source in this install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvokeGrant {
    /// The statement id, as the worker controller names it.
    pub statement_id: String,
    /// The AWS service, such as `s3.amazonaws.com`.
    pub principal: String,
    /// The one source the service may invoke the function for.
    pub source_arn: String,
    /// The account that source must belong to, for sources whose ARN names none (S3).
    pub source_account: Option<String>,
}

/// What the verifier read from its own install for one workload.
#[derive(Debug, Clone, Default)]
pub struct Reads {
    /// Deployer input values, by input id.
    pub deployer_inputs: BTreeMap<String, String>,
    /// Binding values for bindings whose value isn't a name, by resource id.
    pub bindings: BTreeMap<String, String>,
    /// What the cloud says about the resource the workload runs as.
    pub own: OwnResource,
}

/// What the verifier read about the resource a workload runs as, to check the workload's own
/// binding against. Empty before the resource exists.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnResource {
    /// The URLs the cloud serves the resource at.
    pub urls: Vec<String>,
    /// The location the resource is in, where the cloud has one.
    pub location: Option<String>,
}

fn refused(reason: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::Refused {
        reason: reason.into(),
    })
}

/// The names whose values differ between two environments, each once.
pub(crate) fn changed_env<'a>(
    read: &'a BTreeMap<String, String>,
    written: &'a BTreeMap<String, String>,
) -> BTreeSet<&'a String> {
    read.keys()
        .chain(written.keys())
        .filter(|key| read.get(*key) != written.get(*key))
        .collect()
}

/// The function workloads of `release`.
pub fn functions(release: &ReleaseBody) -> impl Iterator<Item = &Workload> {
    release
        .workloads
        .iter()
        .filter(|workload| workload.kind == WorkloadKind::Function)
}

/// Who may invoke the AWS function `function` of signed worker `worker`: the API Gateway API
/// `api` when the worker has a public endpoint, each bucket a storage trigger names and each
/// schedule's EventBridge rule, every source pinned to this account and region. The statement
/// ids and the rule names are the worker controller's. Other clouds take none.
pub(crate) fn invoke_grants(
    place: &Place,
    worker: Option<&alien_core::Worker>,
    function: &str,
    api: Option<&str>,
) -> Result<Vec<InvokeGrant>> {
    let InstallId::Aws {
        account, region, ..
    } = &place.install
    else {
        return match api {
            None => Ok(Vec::new()),
            Some(_) => Err(refused(
                "only an AWS function is fronted by an API Gateway API",
            )),
        };
    };
    let mut grants = Vec::new();
    if let Some(api) = api {
        if !worker.is_some_and(|worker| !worker.public_endpoints.is_empty()) {
            return Err(refused(format!(
                "function {function} has no public endpoint in the signed stack, so no API may invoke it"
            )));
        }
        let plain = !api.is_empty()
            && api.len() <= 32
            && api
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
        if !plain {
            return Err(refused(format!("'{api}' is not an API Gateway API id")));
        }
        grants.push(InvokeGrant {
            statement_id: "ApiGatewayInvoke".to_string(),
            principal: "apigateway.amazonaws.com".to_string(),
            source_arn: format!("arn:aws:execute-api:{region}:{account}:{api}/*"),
            source_account: None,
        });
    }
    for (index, trigger) in worker
        .map(|worker| worker.triggers.as_slice())
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        match trigger {
            alien_core::WorkerTrigger::Storage { storage, .. } => grants.push(InvokeGrant {
                statement_id: format!("{function}-s3-{}", storage.id),
                principal: "s3.amazonaws.com".to_string(),
                source_arn: format!("arn:aws:s3:::{}-{}", place.prefix, storage.id),
                source_account: Some(account.clone()),
            }),
            alien_core::WorkerTrigger::Schedule { .. } => grants.push(InvokeGrant {
                statement_id: format!("{function}-eb-{index}"),
                principal: "events.amazonaws.com".to_string(),
                source_arn: format!(
                    "arn:aws:events:{region}:{account}:rule/{function}-cron-{index}"
                ),
                source_account: None,
            }),
            // A queue reaches the function through an event source mapping, not its policy.
            alien_core::WorkerTrigger::Queue { .. } => {}
        }
    }
    Ok(grants)
}

/// Worker `name` as the release's signed stack declares it, if the release carries the stack.
pub(crate) fn signed_worker(
    release: &ReleaseBody,
    place: &Place,
    name: &str,
) -> Result<Option<alien_core::Worker>> {
    if release.stack.is_none() {
        return Ok(None);
    }
    let platform: alien_core::Platform = place
        .platform()
        .parse()
        .map_err(|error: String| refused(error))?;
    let stack = alien_core::release_statement::signed_stack(release, platform).context(
        ErrorData::Refused {
            reason: "the signed stack does not match the signed workloads".to_string(),
        },
    )?;
    Ok(stack
        .resources
        .get(name)
        .and_then(|entry| entry.config.downcast_ref::<alien_core::Worker>())
        .cloned())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::fixtures::*;
    use crate::trust::accept;
    use crate::trust::tests::{found, MemoryStore};
    use alien_signing::release::SecretBinding;
    use alien_signing::SignedStatement;

    pub fn place() -> Place {
        Place {
            install: alien_signing::InstallId::parse(&install_id()).unwrap(),
            prefix: PREFIX.to_string(),
            vpc: None,
            machine_network: None,
        }
    }

    pub(crate) fn release_with_access(public: bool) -> SignedStatement {
        release_on(alien_core::Platform::Aws, public)
    }

    /// Release 1 of the `api` worker with triggers on `platform`, public or not.
    pub(crate) fn release_on(platform: alien_core::Platform, public: bool) -> SignedStatement {
        let uploads = alien_core::Storage::new("uploads".to_string()).build();
        let jobs = alien_core::Queue::new("jobs".to_string()).build();
        let mut worker = alien_core::Worker::new("api".to_string())
            .code(alien_core::WorkerCode::Image {
                image: format!("ghcr.io/shop/api@{DIGEST}"),
            })
            .permissions("execution".to_string())
            .concurrency_limit(5)
            .build();
        worker.triggers = vec![
            alien_core::WorkerTrigger::storage(&uploads, vec!["created".to_string()]),
            alien_core::WorkerTrigger::queue(&jobs),
            alien_core::WorkerTrigger::schedule("0 * * * *"),
        ];
        if public {
            worker.public_endpoints = vec![alien_core::WorkerPublicEndpoint {
                name: "api".to_string(),
                host_label: None,
                wildcard_subdomains: false,
            }];
        }
        let stack = alien_core::Stack::new("shop".to_string())
            .add(uploads, alien_core::ResourceLifecycle::Frozen)
            .add(jobs, alien_core::ResourceLifecycle::Frozen)
            .add(worker, alien_core::ResourceLifecycle::Live)
            .build();
        let body = alien_core::release_statement::release_body(
            platform,
            &stack,
            &alien_core::release_statement::ReleaseOptions {
                channel: CHANNEL.to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        signed_release(1, &body)
    }

    #[test]
    fn a_gcp_function_runs_as_the_account_setup_creates_for_its_profile() {
        for (prefix, account) in [
            ("acme-prod", "a-acme-prod-execution-cd640cda"),
            ("acme-production-eu", "a-acme-production-eu-7e913b42"),
        ] {
            let place = Place {
                install: alien_signing::InstallId::parse(&format!("gcp:shop-prod:{prefix}"))
                    .unwrap(),
                prefix: prefix.to_string(),
                vpc: None,
                machine_network: None,
            };
            assert_eq!(
                place.run_as("execution"),
                format!("{account}@shop-prod.iam.gserviceaccount.com")
            );
        }
    }

    #[test]
    fn an_aws_function_runs_as_the_role_setup_creates_for_its_profile() {
        for (prefix, profile, role) in [
            ("acme-prod", "execution", "acme-prod-execution-sa"),
            // Over IAM's 64 characters: setup shortens the name and appends a hash.
            (
                "acme-production-eu-west-1-tenant-customer42",
                "background-processor",
                "acme-production-eu-west-1-tenant-customer42-background--894db55f",
            ),
        ] {
            let place = Place {
                install: alien_signing::InstallId::parse(&format!(
                    "aws:111122223333:us-east-1:{prefix}"
                ))
                .unwrap(),
                prefix: prefix.to_string(),
                vpc: None,
                machine_network: None,
            };
            assert_eq!(
                place.run_as(profile),
                format!("arn:aws:iam::111122223333:role/{role}")
            );
        }
    }

    /// A vault value must never decide what code the process loads: a release whose secret
    /// would land in a loader variable is refused before anything is written.
    #[tokio::test]
    async fn a_secret_that_would_set_a_loader_variable_is_refused() {
        let mut workload = function_workload();
        workload.secrets = vec![SecretBinding {
            name: "LD_PRELOAD".into(),
            env_name: Some("LD_PRELOAD".into()),
            file_path: None,
        }];
        let install = found(&MemoryStore::protected(&settings())).await.unwrap();
        let error = accept(
            &install,
            &[signer_list()],
            &release(10, workload),
            None,
            "aws",
            now(),
        )
        .unwrap_err();
        assert!(
            crate::describe(&error)
                .contains("Workload 'api' refused: LD_PRELOAD changes how code loads"),
            "{}",
            crate::describe(&error)
        );
    }
}
