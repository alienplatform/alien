//! The MicroVM image build a sandbox gets, as one function of the signed sandbox and the place.
//!
//! The controller that starts a build and the verifier that checks one derive the request from
//! here, so what the verifier accepts is what the controller sends.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use super::lambda_microvms::{
    CreateMicrovmImageRequest, MicrovmCodeArtifact, MicrovmCpuConfiguration,
    MicrovmImageBuildHooks, MicrovmImageHooks, MicrovmImageLogging, MicrovmImageResources,
    MicrovmLifecycleHooks, UpdateMicrovmImageRequest,
};
use alien_core::sandbox_image::AWS_MICROVM;

/// The only architecture MicroVM images accept.
pub const ARCHITECTURE: &str = "ARM_64";

/// Partition for ARNs minted here, where no CloudFormation pseudo-parameter can resolve it.
pub fn aws_partition(region: &str) -> &'static str {
    if region.starts_with("us-gov-") {
        "aws-us-gov"
    } else if region.starts_with("cn-") {
        "aws-cn"
    } else {
        "aws"
    }
}

/// Assembles the build inputs from the declaration, so a create and a roll of the same sandbox
/// differ only in the bundle.
pub fn image_build_inputs(
    region: &str,
    resource_id: &str,
    image_name: &str,
    build_role_arn: &str,
    bundle_uri: &str,
    baseline_memory_mib: i64,
) -> ImageBuildInputs {
    ImageBuildInputs {
        description: format!("Sandbox {resource_id}"),
        base_image_arn: managed_base_image_arn(region),
        build_role_arn: build_role_arn.to_string(),
        code_artifact: MicrovmCodeArtifact {
            uri: bundle_uri.to_string(),
        },
        egress_network_connectors: vec![internet_egress_connector_arn(region)],
        resources: vec![MicrovmImageResources {
            minimum_memory_in_mib: baseline_memory_mib,
        }],
        // Keyed on the account-unique image name and the bundle: a retry of the same release
        // replays the prior success, a new release's bundle asks for a real build, and another
        // deployment in the same account rolling the same bundle onto its own image never reuses
        // this image's token.
        client_token: build_client_token(image_name, bundle_uri),
        environment: agent_environment(),
        additional_os_capabilities: vec![],
    }
}

/// The build inputs a create and a roll must state identically.
///
/// `UpdateMicrovmImage` has PUT semantics — a field left out of the roll is dropped from the
/// new version — so the two requests are built from one value rather than assembled twice.
#[derive(Clone)]
pub struct ImageBuildInputs {
    pub description: String,
    pub base_image_arn: String,
    pub build_role_arn: String,
    pub code_artifact: MicrovmCodeArtifact,
    pub egress_network_connectors: Vec<String>,
    pub resources: Vec<MicrovmImageResources>,
    pub client_token: String,
    pub environment: BTreeMap<String, String>,
    pub additional_os_capabilities: Vec<String>,
}

impl ImageBuildInputs {
    pub fn with_agent_environment(mut self, environment: BTreeMap<String, String>) -> Self {
        self.environment.extend(environment);
        if self.environment.contains_key("ALIEN_SANDBOX_EGRESS")
            || self.environment.contains_key("ALIEN_SANDBOX_IMAGE_COMMAND")
        {
            // AWS rejects named capabilities (including NET_ADMIN): the API enum is only ALL.
            // The agent reduces its grant before starting threads, bounds root image startup,
            // and gives exec no capabilities. The grant also permits firewall installation.
            // https://docs.aws.amazon.com/lambda/latest/dg/microvms-images.html#microvms-images-os-capabilities
            self.additional_os_capabilities = vec!["ALL".to_string()];
            // Fold the complete policy into the token without truncating away its digest.
            let digest =
                Sha256::digest(format!("{}{:?}", self.client_token, self.environment).as_bytes());
            self.client_token = format!("agent-{:x}", digest)[..64].to_string();
        }
        self
    }

    pub fn for_roll(mut self, previous_version: Option<&str>) -> Self {
        self.client_token = roll_client_token(&self.client_token, previous_version);
        self
    }

    pub fn for_retry(mut self, generation: u64) -> Self {
        if generation != 0 {
            let digest = Sha256::digest(format!("{}:retry:{generation}", self.client_token));
            self.client_token = format!("{digest:x}");
        }
        self
    }

    /// The receipt hashes the whole request, so a release that changes any field of it (hooks,
    /// environment, base image) does not recognise a build an earlier release started and lost
    /// the response to: recovery misses it and builds again, leaving that version untracked.
    /// Accepted for the narrow window it needs, a lost response retried across an upgrade.
    pub fn with_receipt(mut self) -> serde_json::Result<Self> {
        let encoded = serde_json::to_vec(&self.clone().update_request())?;
        self.client_token = format!("{:x}", Sha256::digest(encoded));
        self.description = format!("{} [build:{}]", self.description, self.client_token);
        Ok(self)
    }

    pub fn create_request(
        self,
        name: String,
        tags: BTreeMap<String, String>,
    ) -> CreateMicrovmImageRequest {
        CreateMicrovmImageRequest::builder()
            .name(name)
            .description(self.description)
            .base_image_arn(self.base_image_arn)
            .base_image_version("1")
            .build_role_arn(self.build_role_arn)
            .code_artifact(self.code_artifact)
            // The switch behind "control plane never sees sandbox contents".
            .logging(MicrovmImageLogging::Disabled {})
            .egress_network_connectors(self.egress_network_connectors)
            .cpu_configurations(vec![MicrovmCpuConfiguration {
                architecture: ARCHITECTURE.to_string(),
            }])
            .resources(self.resources)
            .hooks(agent_hooks())
            .environment_variables(self.environment)
            .additional_os_capabilities(self.additional_os_capabilities)
            // The deployed `sandbox/provision` grant conditions `CreateMicrovmImage` on the
            // stack tag and `managed-by: runtime` arriving as request tags; without them the
            // call is denied by a policy that is already installed.
            .tags(tags)
            .client_token(self.client_token)
            .build()
    }

    /// The roll carries no tags: the API rejects them here, and the image keeps the ones its
    /// create set — which is what every other sandbox statement scopes against.
    pub fn update_request(self) -> UpdateMicrovmImageRequest {
        UpdateMicrovmImageRequest::builder()
            .description(self.description)
            .base_image_arn(self.base_image_arn)
            .base_image_version("1")
            .build_role_arn(self.build_role_arn)
            .code_artifact(self.code_artifact)
            .logging(MicrovmImageLogging::Disabled {})
            .egress_network_connectors(self.egress_network_connectors)
            .cpu_configurations(vec![MicrovmCpuConfiguration {
                architecture: ARCHITECTURE.to_string(),
            }])
            .resources(self.resources)
            .hooks(agent_hooks())
            .environment_variables(self.environment)
            .additional_os_capabilities(self.additional_os_capabilities)
            .client_token(self.client_token)
            .build()
    }
}

/// A build idempotency token that changes with the bundle. Both the create and the roll replay a
/// prior success for a repeated token, so folding the content-addressed bundle in keeps a retry
/// idempotent while a genuinely new release asks for a real build.
pub fn build_client_token(image_name: &str, bundle_uri: &str) -> String {
    let digest = Sha256::digest(bundle_uri.as_bytes());
    // Sixteen hex chars of the bundle digest is plenty to separate one release from the next,
    // and keeps the token well under the API's length ceiling.
    format!("{image_name}-{:x}", digest)
        .chars()
        .take(image_name.len() + 1 + 16)
        .collect()
}

pub fn roll_client_token(token: &str, previous_version: Option<&str>) -> String {
    let digest = Sha256::digest(format!("{token}:{previous_version:?}").as_bytes());
    format!("roll-{:x}", digest)[..64].to_string()
}

/// The pre-create probe has no ARN to adopt yet, and the API answers a bare name with a 400
/// that no absent-resource check can read as absence. The name is account-unique, so the ARN
/// it will carry is derivable before the image exists.
pub fn sandbox_image_arn(region: &str, account_id: &str, image_name: &str) -> String {
    format!(
        "arn:{}:lambda:{region}:{account_id}:microvm-image:{image_name}",
        aws_partition(region)
    )
}

/// The Lambda-managed base image the build runs on. The vendor's own base is inside the
/// bundle's Dockerfile, not here.
pub fn managed_base_image_arn(region: &str) -> String {
    format!(
        "arn:{}:lambda:{region}:aws:microvm-image:al2023-1",
        aws_partition(region)
    )
}

/// AWS's own internet connector, which the image build routes through to reach a registry.
pub fn internet_egress_connector_arn(region: &str) -> String {
    format!(
        "arn:{}:lambda:{region}:aws:network-connector:aws-network-connector:INTERNET_EGRESS",
        aws_partition(region)
    )
}

/// Lifecycle hooks, served by the agent on its own port; mirrors what the setup emitters bake
/// into a Frozen image.
///
/// `Ready` is not optional — AWS rejects an image enabling any MicroVM hook without it, and it
/// is what defers the snapshot until the agent is serving. `Validate` runs on a MicroVM restored
/// from the snapshot and fails the build when a command or a file write cannot work on it; 30s
/// is AWS's default and far above the probe's sub-second work, where `Ready`'s 120s covers a
/// boot. `Run` and `Resume` are acknowledged by the agent and nothing more: entropy separation
/// between MicroVMs comes from the MicroVM service.
pub fn agent_hooks() -> MicrovmImageHooks {
    MicrovmImageHooks::builder()
        .port(AWS_MICROVM.port)
        .microvm_image_hooks(MicrovmImageBuildHooks {
            ready: "ENABLED".to_string(),
            ready_timeout_in_seconds: 120,
            validate: "ENABLED".to_string(),
            validate_timeout_in_seconds: 30,
        })
        .microvm_hooks(MicrovmLifecycleHooks {
            run: "ENABLED".to_string(),
            run_timeout_in_seconds: 30,
            resume: "ENABLED".to_string(),
            resume_timeout_in_seconds: 30,
        })
        .build()
}

/// The agent's configuration contract, identical to the setup emitters' rendering.
///
/// Identity and isolation come from the image: runtime defaults would overwrite its USER.
pub fn agent_environment() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "ALIEN_SANDBOX_ROOT".to_string(),
            AWS_MICROVM.session_root.to_string(),
        ),
        (
            "ALIEN_SANDBOX_PORT".to_string(),
            AWS_MICROVM.port.to_string(),
        ),
        (
            "ALIEN_SANDBOX_AUTHORIZATION".to_string(),
            AWS_MICROVM.authorization.env_value().to_string(),
        ),
    ])
}
