//! From an accepted release to the functions the verifier writes.
//!
//! Nothing here touches the network: the caller supplies the release it
//! accepted, the deployer inputs and binding values it read from its own
//! install, and gets back each function exactly as it must run.

use std::collections::{BTreeMap, BTreeSet};

use alien_core::{aws_naming::aws_iam_role_name, gcp_naming::gcp_service_account_id};
use alien_error::{AlienError, Context};
use alien_signing::release::{is_sha256_digest, Runtime, WorkloadKind};
use alien_signing::{
    binding_env, build_workload, resolve_bindings, BindingContext, InstallId, LocalValues,
    ReleaseBody, SignedStatement, Workload, WorkloadRequest,
};
use serde::{Deserialize, Serialize};

use crate::error::{ErrorData, Result};
use crate::trust::Accepted;

/// Environment variable the verifier writes with the secrets a protected
/// function loads at start. Unprotected functions keep `ALIEN_SECRETS`,
/// which the engine writes; the control plane can set neither here.
pub const SECRETS_ENV: &str = "WORKLOAD_SECRETS";

/// A call to the verifier. Unknown fields are refused: the control plane
/// passes signed statements and the values the release names as provided,
/// never a function spec.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum Request {
    /// Write every function the release names, then read each one back.
    ApplyRelease(ReleaseRequest),
    /// Only read every function back and report whether it matches.
    CheckRelease(ReleaseRequest),
    /// Roll a machine cluster to the release's launch template (AWS).
    RollMachines(crate::machines::RollMachines),
    /// Resize a machine cluster within its signed bounds (GCP, Azure).
    Scale(crate::machines::Scale),
    /// Delete named machines of a group, never below its signed minimum
    /// (GCP, Azure).
    RemoveMachines(crate::machines::RemoveMachines),
}

/// The statements and values for one release.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseRequest {
    /// The release, as the vendor signed it.
    pub release: SignedStatement,
    /// The signer list chain, from version 1.
    pub signer_lists: Vec<SignedStatement>,
    /// This install's config, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<SignedStatement>,
    /// Values of the env names the release lists as provided, by workload.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub provided_env: BTreeMap<String, BTreeMap<String, String>>,
    /// Where this install's copy of each workload's image lives, by
    /// workload. The digest must be the signed one.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub images: BTreeMap<String, String>,
    /// The function workloads to write or read back, when not every one: a
    /// control plane that renders each function on its own sends that
    /// function's provided values alone, and the others keep theirs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workloads: Option<BTreeSet<String>>,
    /// Credentials for the registry the images are pulled from, where the cloud needs them on
    /// the function (Azure Container Apps). They can't change what runs: the image is pinned by
    /// the signed digest. Only for the registry the images name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry: Option<RegistryCredentials>,
    /// AWS: the API Gateway API in front of each workload, by workload, once management created
    /// it. Only a workload the signed stack gives a public endpoint may name one; the API may
    /// then invoke the function.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub apis: BTreeMap<String, String>,
}

/// A registry's pull credentials.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegistryCredentials {
    /// Registry host, as the image references name it.
    pub server: String,
    /// User name.
    pub username: String,
    /// Password or token.
    pub password: String,
}

impl std::fmt::Debug for RegistryCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegistryCredentials")
            .field("server", &self.server)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

impl ReleaseRequest {
    /// Whether this request writes or reads back `workload`.
    pub fn selects(&self, workload: &str) -> bool {
        self.workloads
            .as_ref()
            .is_none_or(|selected| selected.contains(workload))
    }
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
    fn default_architecture(&self) -> &'static str {
        match self.install {
            InstallId::Aws { .. } => "arm64",
            _ => "x86_64",
        }
    }
}

/// A function the verifier writes, built from the signed workload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionSpec {
    /// Workload name in the release.
    pub workload: String,
    /// Function name.
    pub name: String,
    /// Image URI, pinned by digest.
    pub image_uri: String,
    /// The signed digest.
    pub digest: String,
    /// The whole environment.
    pub env: BTreeMap<String, String>,
    /// Memory in MiB.
    pub memory_mb: u32,
    /// Timeout in seconds.
    pub timeout_seconds: u32,
    /// `x86_64` or `arm64`.
    pub architecture: String,
    /// What it runs as: the execution role ARN on AWS, the service account
    /// email on GCP, the user-assigned identity resource id on Azure.
    pub run_as: String,
    /// The VPC setup fixed for functions, if any.
    pub vpc: Option<SetupNetwork>,
    /// Whether the signed stack gives the workload a public endpoint. A function the verifier
    /// creates takes requests from the internet only then.
    pub public: bool,
    /// Whether the signed stack gives the workload triggers or commands, which reach a Container
    /// App through Dapr.
    pub dapr: bool,
    /// Credentials for the registry the image is pulled from, when the request carried them.
    #[serde(skip)]
    pub registry: Option<RegistryCredentials>,
    /// AWS: the reserved concurrency the signed stack sets, if any.
    pub reserved_concurrency: Option<u32>,
    /// AWS: who may invoke the function, from the signed stack's endpoints and triggers. The
    /// function's resource policy holds exactly these.
    pub invoke_grants: Vec<InvokeGrant>,
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

/// Build the function of `workload` from an accepted release.
pub fn plan_function(
    place: &Place,
    accepted: &Accepted,
    request: &ReleaseRequest,
    workload: &Workload,
    reads: &Reads,
) -> Result<FunctionSpec> {
    let name = &workload.name;
    let settings = workload
        .function
        .as_ref()
        .ok_or_else(|| refused(format!("workload '{name}' has no signed function settings")))?;
    // The verifier writes no image config, layers or mounts, so a release
    // that needs them is refused rather than written without them.
    if workload.command.is_some() {
        return Err(refused(format!(
            "workload '{name}' overrides the image command, which a function can't"
        )));
    }
    if workload.runtime != Runtime::default() || workload.volume_mount_path.is_some() {
        return Err(refused(format!(
            "workload '{name}' asks for runtime privileges or mounts a function can't have"
        )));
    }
    let profile = workload
        .identity
        .as_deref()
        .ok_or_else(|| refused(format!("workload '{name}' names no permission profile")))?;

    let config = accepted.config_body();
    let derived =
        binding_env(&workload.bindings, &place.binding_context()).context(ErrorData::Refused {
            reason: format!("the bindings of workload '{name}' were refused"),
        })?;
    let bindings = resolve_bindings(derived, &reads.bindings, &config.bindings).context(
        ErrorData::Refused {
            reason: format!("the bindings of workload '{name}' have no value"),
        },
    )?;
    let mut built = build_workload(
        &accepted.release.body,
        name,
        &config.env,
        &LocalValues {
            deployer_inputs: reads.deployer_inputs.clone(),
            bindings,
        },
        &WorkloadRequest {
            env: request.provided_env.get(name).cloned().unwrap_or_default(),
            ..Default::default()
        },
    )
    .context(ErrorData::Refused {
        reason: "the control plane's values don't fit the release".to_string(),
    })?;

    if !workload.secrets.is_empty() {
        let mut keys = Vec::new();
        for secret in &workload.secrets {
            if secret.env_name.as_deref() != Some(secret.name.as_str())
                || secret.file_path.is_some()
            {
                return Err(refused(format!(
                    "secret '{}' of workload '{name}' must land in env {0} on a function",
                    secret.name
                )));
            }
            keys.push(secret.name.clone());
        }
        let value = serde_json::json!({ "keys": keys, "hash": accepted.release.digest });
        if built
            .env
            .insert(SECRETS_ENV.to_string(), value.to_string())
            .is_some()
        {
            return Err(refused(format!(
                "workload '{name}' sets {SECRETS_ENV} itself, which the verifier writes"
            )));
        }
    }

    let (_, digest) = built
        .image
        .rsplit_once('@')
        .filter(|(_, digest)| is_sha256_digest(digest))
        .ok_or_else(|| refused(format!("image {} is not pinned", built.image)))?;
    let image_uri = match request.images.get(name) {
        None => built.image.clone(),
        Some(copy) => match copy.rsplit_once('@') {
            Some((repository, copy_digest)) if !repository.is_empty() && copy_digest == digest => {
                copy.clone()
            }
            _ => {
                return Err(refused(format!(
                    "image {copy} is not the signed digest {digest}"
                )))
            }
        },
    };
    let architecture = match settings.architecture.as_deref() {
        None => place.default_architecture(),
        Some("x86_64") | Some("amd64") => "x86_64",
        Some("arm64") => "arm64",
        Some(other) => return Err(refused(format!("architecture '{other}' is not known"))),
    };

    let signed = signed_worker(&accepted.release.body, place, name)?;
    let public = signed
        .as_ref()
        .is_some_and(|worker| !worker.public_endpoints.is_empty());
    let dapr = signed
        .as_ref()
        .is_some_and(|worker| worker.commands_enabled || !worker.triggers.is_empty());
    let registry = registry_for(request, &image_uri)?;
    let name_in_cloud = place.function_name(name)?;
    let invoke_grants = invoke_grants(
        place,
        signed.as_ref(),
        &name_in_cloud,
        request.apis.get(name).map(String::as_str),
    )?;
    Ok(FunctionSpec {
        workload: name.clone(),
        name: name_in_cloud,
        image_uri,
        digest: digest.to_string(),
        env: std::mem::take(&mut built.env),
        memory_mb: settings.memory_mb,
        timeout_seconds: settings.timeout_seconds,
        architecture: architecture.to_string(),
        run_as: place.run_as(profile),
        vpc: place.vpc.clone(),
        public,
        dapr,
        registry,
        reserved_concurrency: signed.as_ref().and_then(|worker| worker.concurrency_limit),
        invoke_grants,
    })
}

/// Who may invoke the AWS function `function` of signed worker `worker`: the API Gateway API
/// `api` when the worker has a public endpoint, each bucket a storage trigger names and each
/// schedule's EventBridge rule, every source pinned to this account and region. The statement
/// ids and the rule names are the worker controller's. Other clouds take none.
fn invoke_grants(
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
            Some(_) => Err(refused("only an AWS function is fronted by an API Gateway API")),
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
                source_arn: format!("arn:aws:events:{region}:{account}:rule/{function}-cron-{index}"),
                source_account: None,
            }),
            // A queue reaches the function through an event source mapping, not its policy.
            alien_core::WorkerTrigger::Queue { .. } => {}
        }
    }
    Ok(grants)
}

/// The request's registry credentials, if it carried any, for the registry `image` names. Ones
/// for another registry are refused, so they never go anywhere the image doesn't come from.
fn registry_for(request: &ReleaseRequest, image: &str) -> Result<Option<RegistryCredentials>> {
    let Some(registry) = &request.registry else {
        return Ok(None);
    };
    let host = image.split('/').next().unwrap_or_default();
    if registry.server != host {
        return Err(refused(format!(
            "registry credentials are for '{}', but the image comes from '{host}'",
            registry.server
        )));
    }
    Ok(Some(registry.clone()))
}

/// Worker `name` as the release's signed stack declares it, if the release carries the stack.
fn signed_worker(
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
    use crate::trust::tests::{found, MemoryStore};
    use crate::trust::{accept, Install};
    use alien_signing::release::SecretBinding;
    use alien_signing::ConfigBody;

    pub fn place() -> Place {
        Place {
            install: alien_signing::InstallId::parse(&install_id()).unwrap(),
            prefix: PREFIX.to_string(),
            vpc: None,
            machine_network: None,
        }
    }

    pub fn request(release: SignedStatement) -> ReleaseRequest {
        ReleaseRequest {
            release,
            signer_lists: vec![signer_list()],
            config: None,
            provided_env: BTreeMap::from([(
                "api".to_string(),
                BTreeMap::from([("ALIEN_DEPLOYMENT_ID".to_string(), "dep_1".to_string())]),
            )]),
            images: BTreeMap::from([("api".to_string(), format!("{REGISTRY}/api@{DIGEST}"))]),
            workloads: None,
            registry: None,
            apis: BTreeMap::new(),
        }
    }

    async fn install() -> Install {
        found(&MemoryStore::protected(&settings())).await.unwrap()
    }

    pub async fn planned(request: &ReleaseRequest, reads: &Reads) -> Result<FunctionSpec> {
        let install = install().await;
        let accepted = accept(
            &install,
            &request.signer_lists,
            &request.release,
            request.config.as_ref(),
            "aws",
            now(),
        )?;
        let workload = functions(&accepted.release.body)
            .next()
            .expect("one function")
            .clone();
        plan_function(&place(), &accepted, request, &workload, reads)
    }

    async fn refused(request: &ReleaseRequest) -> String {
        crate::describe(&planned(request, &Reads::default()).await.unwrap_err())
    }

    #[tokio::test]
    async fn the_function_is_built_from_the_signed_workload() {
        let spec = planned(
            &request(release(10, function_workload())),
            &Reads::default(),
        )
        .await
        .unwrap();
        assert_eq!(
            spec,
            FunctionSpec {
                workload: "api".into(),
                name: "acme-prod-api".into(),
                image_uri: format!("{REGISTRY}/api@{DIGEST}"),
                digest: DIGEST.into(),
                env: BTreeMap::from([
                    ("ALIEN_DEPLOYMENT_ID".into(), "dep_1".into()),
                    ("MODE".into(), "signed".into()),
                ]),
                memory_mb: 512,
                timeout_seconds: 30,
                architecture: "arm64".into(),
                run_as: "arn:aws:iam::111122223333:role/acme-prod-execution-sa".into(),
                vpc: None,
                public: false,
                dapr: false,
                registry: None,
                reserved_concurrency: None,
                invoke_grants: Vec::new(),
            }
        );
    }

    /// Release 1 of a stack whose worker `api` reserves 5, is fronted by a public endpoint when
    /// `public`, and is triggered by bucket `uploads`, a queue and a schedule.
    fn release_with_access(public: bool) -> SignedStatement {
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
            alien_core::Platform::Aws,
            &stack,
            &alien_core::release_statement::ReleaseOptions {
                channel: CHANNEL.to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        signed_release(1, &body)
    }

    #[tokio::test]
    async fn who_may_invoke_a_function_and_how_often_come_from_the_signed_stack() {
        let mut request = request(release_with_access(true));
        request.images.clear();
        request.provided_env.clear();
        request.apis = BTreeMap::from([("api".to_string(), "a1b2c3d4e5".to_string())]);
        let spec = planned(&request, &Reads::default()).await.unwrap();
        assert_eq!(spec.reserved_concurrency, Some(5));
        assert_eq!(
            spec.invoke_grants,
            vec![
                InvokeGrant {
                    statement_id: "ApiGatewayInvoke".to_string(),
                    principal: "apigateway.amazonaws.com".to_string(),
                    source_arn: format!("arn:aws:execute-api:{REGION}:{ACCOUNT}:a1b2c3d4e5/*"),
                    source_account: None,
                },
                InvokeGrant {
                    statement_id: format!("{PREFIX}-api-s3-uploads"),
                    principal: "s3.amazonaws.com".to_string(),
                    source_arn: format!("arn:aws:s3:::{PREFIX}-uploads"),
                    source_account: Some(ACCOUNT.to_string()),
                },
                InvokeGrant {
                    statement_id: format!("{PREFIX}-api-eb-2"),
                    principal: "events.amazonaws.com".to_string(),
                    source_arn: format!("arn:aws:events:{REGION}:{ACCOUNT}:rule/{PREFIX}-api-cron-2"),
                    source_account: None,
                },
            ]
        );

        // Before management created the API, the function takes no API grant.
        request.apis.clear();
        let spec = planned(&request, &Reads::default()).await.unwrap();
        assert!(spec
            .invoke_grants
            .iter()
            .all(|grant| grant.principal != "apigateway.amazonaws.com"));
    }

    #[tokio::test]
    async fn an_api_may_front_only_a_public_function_and_only_by_its_id() {
        let mut private = request(release_with_access(false));
        private.images.clear();
        private.provided_env.clear();
        private.apis = BTreeMap::from([("api".to_string(), "a1b2c3d4e5".to_string())]);
        assert!(refused(&private).await.contains("has no public endpoint"));

        let mut public = request(release_with_access(true));
        public.images.clear();
        public.provided_env.clear();
        public.apis = BTreeMap::from([("api".to_string(), "*".to_string())]);
        assert!(refused(&public).await.contains("is not an API Gateway API id"));
    }

    #[tokio::test]
    async fn registry_credentials_go_only_to_the_registry_the_image_comes_from() {
        let mut request = request(release(10, function_workload()));
        let host = REGISTRY.split('/').next().unwrap().to_string();
        request.registry = Some(RegistryCredentials {
            server: host.clone(),
            username: "deployment".to_string(),
            password: "token".to_string(),
        });
        let spec = planned(&request, &Reads::default()).await.unwrap();
        assert_eq!(
            spec.registry
                .as_ref()
                .map(|registry| registry.server.as_str()),
            Some(host.as_str())
        );
        assert!(
            !format!("{spec:?}").contains("token"),
            "the password never prints"
        );

        request.registry.as_mut().unwrap().server = "attacker.example.com".to_string();
        let reason = refused(&request).await;
        assert!(
            reason.contains("registry credentials are for 'attacker.example.com'"),
            "{reason}"
        );
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

    #[test]
    fn a_spec_from_the_control_plane_is_refused() {
        let mut value = serde_json::to_value(Request::ApplyRelease(request(release(
            10,
            function_workload(),
        ))))
        .unwrap();
        value["applyRelease"]["environment"] = serde_json::json!({"Variables": {"MODE": "x"}});
        let error = serde_json::from_value::<Request>(value).unwrap_err();
        assert!(
            error.to_string().contains("unknown field `environment`"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn values_the_release_does_not_allow_are_refused() {
        for (name, value) in [
            ("MODE", "attacker"),
            ("NODE_OPTIONS", "--require /tmp/x.js"),
            ("LOG_LEVEL", "trace"),
            ("WORKLOAD_SECRETS", r#"{"keys":["LD_PRELOAD"],"hash":"x"}"#),
            ("ALIEN_SECRETS", r#"{"keys":["LD_PRELOAD"],"hash":"x"}"#),
        ] {
            let mut changed = request(release(10, function_workload()));
            changed
                .provided_env
                .get_mut("api")
                .unwrap()
                .insert(name.into(), value.into());
            let error = refused(&changed).await;
            assert!(error.contains("don't fit the release"), "{name}: {error}");
        }
    }

    #[tokio::test]
    async fn only_the_signed_digest_is_written() {
        let other = format!("sha256:{}", "2".repeat(64));
        let mut digest = request(release(10, function_workload()));
        digest
            .images
            .insert("api".into(), format!("{REGISTRY}/api@{other}"));
        assert!(refused(&digest).await.contains("is not the signed digest"));
        let mut tag = request(release(10, function_workload()));
        tag.images
            .insert("api".into(), format!("{REGISTRY}/api:latest"));
        assert!(refused(&tag).await.contains("is not the signed digest"));
    }

    #[tokio::test]
    async fn bindings_come_from_the_install_never_the_control_plane() {
        let mut workload = function_workload();
        workload.bindings = BTreeMap::from([
            ("uploads".to_string(), "storage".to_string()),
            ("db".to_string(), "postgres".to_string()),
            ("search".to_string(), "worker".to_string()),
        ]);
        let reads = Reads {
            bindings: BTreeMap::from([(
                "search".to_string(),
                r#"{"service":"lambda","workerName":"acme-prod-search","region":"us-east-1"}"#
                    .to_string(),
            )]),
            ..Reads::default()
        };
        let mut with_config = request(release(10, workload.clone()));
        with_config.config = Some(config(
            3,
            ConfigBody {
                bindings: BTreeMap::from([(
                    "db".to_string(),
                    serde_json::json!({"service": "external", "host": "db.vendor.example"}),
                )]),
                ..ConfigBody::default()
            },
        ));
        let spec = planned(&with_config, &reads).await.unwrap();
        // Compared as JSON: key order depends on serde_json's features.
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&spec.env["ALIEN_UPLOADS_BINDING"]).unwrap(),
            serde_json::json!({"bucketName": "acme-prod-uploads", "service": "s3"})
        );
        assert!(spec.env["ALIEN_SEARCH_BINDING"].contains("acme-prod-search"));
        assert!(spec.env["ALIEN_DB_BINDING"].contains("db.vendor.example"));

        // The control plane pointing each one elsewhere is refused.
        for (env, value) in [
            (
                "ALIEN_UPLOADS_BINDING",
                r#"{"service":"s3","bucketName":"other-customer"}"#,
            ),
            (
                "ALIEN_SEARCH_BINDING",
                r#"{"service":"lambda","workerName":"evil","region":"us-east-1"}"#,
            ),
            (
                "ALIEN_DB_BINDING",
                r#"{"service":"external","host":"evil.example"}"#,
            ),
        ] {
            let mut attack = with_config.clone();
            attack
                .provided_env
                .get_mut("api")
                .unwrap()
                .insert(env.into(), value.into());
            let error = crate::describe(&planned(&attack, &reads).await.unwrap_err());
            assert!(
                error.contains(&format!("env {env} differs")),
                "{env}: {error}"
            );
        }

        // Without a config, the binding the install can't supply is refused.
        let error = crate::describe(
            &planned(&request(release(10, workload)), &reads)
                .await
                .unwrap_err(),
        );
        assert!(
            error.contains("binding 'db' (postgres) has no value"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn the_secrets_list_is_built_from_the_signed_workload() {
        let mut workload = function_workload();
        workload.secrets = vec![SecretBinding {
            name: "DB_PASSWORD".into(),
            env_name: Some("DB_PASSWORD".into()),
            file_path: None,
        }];
        let signed = release(10, workload);
        let digest = signed.open().unwrap().digest;
        let spec = planned(&request(signed), &Reads::default()).await.unwrap();
        assert_eq!(
            spec.env[SECRETS_ENV],
            serde_json::json!({"keys": ["DB_PASSWORD"], "hash": digest}).to_string()
        );
        assert!(!spec.env.contains_key("ALIEN_SECRETS"));
    }

    #[tokio::test]
    async fn a_secret_that_would_set_a_loader_variable_is_refused() {
        let mut workload = function_workload();
        workload.secrets = vec![SecretBinding {
            name: "LD_PRELOAD".into(),
            env_name: Some("LD_PRELOAD".into()),
            file_path: None,
        }];
        let error = refused(&request(release(10, workload))).await;
        // A vault value must never decide what code the process loads; the
        // release itself is refused.
        assert!(
            error.contains("Workload 'api' refused: LD_PRELOAD changes how code loads"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn deployer_inputs_come_from_the_install() {
        let mut workload = function_workload();
        workload
            .deployer_env
            .insert("REGION_LABEL".into(), "region".into());
        let mut request = request(release(10, workload));
        request
            .provided_env
            .get_mut("api")
            .unwrap()
            .insert("REGION_LABEL".into(), "attacker".into());
        let spec = planned(
            &request,
            &Reads {
                deployer_inputs: BTreeMap::from([("region".into(), "eu".into())]),
                ..Reads::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(spec.env["REGION_LABEL"], "eu");
    }
}
