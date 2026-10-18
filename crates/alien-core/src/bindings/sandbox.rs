//! Sandbox binding definitions.
//!
//! Carries what a provider needs to reach the durable parent and create sandboxes inside it.
//! Sandbox identity is never in here — sandboxes are created at runtime and the provider is the
//! record, so a binding describes the parent only.

use super::BindingValue;
use crate::{EgressPolicy, SandboxEgress};
use serde::{Deserialize, Serialize};

/// Represents a sandbox binding for creating and reaching sandboxes.
///
/// Service tags are prefixed with `sandbox-` because serde selects the variant on the `service`
/// field alone. An unprefixed `local` would deserialize as another resource's local binding by
/// silently dropping the fields that differ.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "service")]
pub enum SandboxBinding {
    /// AWS Lambda MicroVM sandboxes
    #[serde(rename = "sandbox-aws")]
    Aws(AwsSandboxBinding),
    /// Azure Container Apps Sandboxes
    #[serde(rename = "sandbox-azure")]
    Azure(AzureSandboxBinding),
    /// GCP Agent Platform sandboxes, created under a durable Agent Engine
    #[serde(rename = "sandbox-gcp-agent-platform")]
    GcpAgentPlatform(GcpAgentPlatformSandboxBinding),
    /// Sandbox pods under a sandboxed runtime class
    #[serde(rename = "sandbox-kubernetes")]
    Kubernetes(KubernetesSandboxBinding),
    /// Local Docker sandboxes managed by the local sandbox manager
    #[serde(rename = "sandbox-local")]
    Local(LocalSandboxBinding),
}

/// Gateway handoff, or a policy-only binding from deployments without a gateway.
/// Policy-only bindings remain readable but cannot enforce per-sandbox changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum SandboxEgressBinding {
    /// Complete gateway configuration from the deployment controller.
    Gateway(SandboxEgressGateway),
    /// Legacy declaration; no runtime enforcement capability is implied.
    Policy(SandboxEgress),
    /// Default policy for a new Internet declaration, retained in controller state.
    Defaults {
        #[serde(rename = "defaultPolicy")]
        default_policy: EgressPolicy,
    },
}

impl SandboxEgressBinding {
    /// Declaration used for validation before any backend call.
    pub fn declaration(&self) -> &SandboxEgress {
        match self {
            Self::Gateway(_) => &SandboxEgress::Internet,
            Self::Policy(value) => value,
            Self::Defaults { .. } => &SandboxEgress::Internet,
        }
    }
    /// Default runtime policy for an Internet declaration.
    pub fn default_policy(&self) -> Option<&EgressPolicy> {
        match self {
            Self::Defaults { default_policy } => Some(default_policy),
            _ => None,
        }
    }
    /// Gateway configuration, absent on older deployments.
    pub fn gateway(&self) -> Option<&SandboxEgressGateway> {
        match self {
            Self::Gateway(value) => Some(value),
            Self::Policy(_) | Self::Defaults { .. } => None,
        }
    }
}

impl<'de> Deserialize<'de> for SandboxEgressBinding {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Input {
            Gateway(SandboxEgressGateway),
            Policy(SandboxEgress),
            Defaults {
                #[serde(rename = "defaultPolicy")]
                default_policy: EgressPolicy,
            },
        }
        Ok(match Input::deserialize(deserializer)? {
            Input::Gateway(value) => Self::Gateway(value),
            Input::Policy(value) => Self::Policy(value),
            Input::Defaults { default_policy } => Self::Defaults { default_policy },
        })
    }
}
impl From<&crate::Sandbox> for SandboxEgressBinding {
    fn from(value: &crate::Sandbox) -> Self {
        match &value.default_policy {
            Some(default_policy) => Self::Defaults {
                default_policy: default_policy.clone(),
            },
            None => Self::Policy(value.egress.clone()),
        }
    }
}

impl From<SandboxEgress> for SandboxEgressBinding {
    fn from(value: SandboxEgress) -> Self {
        Self::Policy(value)
    }
}

/// Public binding metadata. Private key material never belongs in a binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SandboxEgressGateway {
    /// Linked gateway Container binding; its internal URL is resolved at runtime.
    pub container: String,
    /// Deployment prefix and resource id.
    pub audience: String,
    /// Vault locator for the deployment's signing key.
    pub signing_key: SandboxEgressSigningKey,
    /// Key id published in the gateway's rotation set.
    pub kid: String,
    /// Trust root for an admin listener using a deployment-issued TLS certificate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admin_ca_pem: Option<String>,
}

/// Management-only vault reference for an Ed25519 PKCS#8 key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SandboxEgressSigningKey {
    /// Name of the vault binding, normally `secrets`.
    pub vault: String,
    /// Secret name, normally `sandbox-egress-signing-key`.
    pub key: String,
}

/// AWS sandbox binding configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsSandboxBinding {
    /// Default runtime policy when create omits one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_policy: Option<EgressPolicy>,
    /// Declared internet access. Absent only in bindings created by older releases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub egress: Option<SandboxEgressBinding>,
    /// MicroVM image ARN that scopes the sandboxes this binding creates
    pub image_arn: BindingValue<String>,
    /// Image version. Sandboxes are enumerated by image and version together, so a rolled
    /// version remains a cleanup scope until its own MicroVMs are gone.
    pub image_version: BindingValue<String>,
    /// Region the MicroVMs run in
    pub region: BindingValue<String>,
    /// Execution role attached to each MicroVM, distinct from the workload's own role
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_role_arn: Option<BindingValue<String>>,
    /// Egress connectors every sandbox is started with.
    ///
    /// Carried rather than implied: a MicroVM started with no connector reaches the public
    /// internet, so an empty list here is `allow`, not `deny`. The declared mode is realised by
    /// which connector setup built, and the sandbox has to be started with it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub egress_connector_arns: Vec<BindingValue<String>>,
    /// Ports a preview capability may be minted for.
    ///
    /// Carried because the token is what grants ingress: `CreateMicrovmAuthToken` mints access to
    /// whatever port it is asked for, so "a port not listed here can never be exposed" is only
    /// true if the declared list reaches the code that mints.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preview_ports: Vec<u16>,
    /// Idle seconds after which a sandbox pauses, if the declaration asked for one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_pause_seconds: Option<u32>,
    /// Wall-clock ceiling on a sandbox, if the declaration asked for one.
    ///
    /// Enforced by Lambda rather than by us: `RunMicrovm` takes it as
    /// `maximumDurationInSeconds` and terminates the MicroVM when it elapses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_lifetime_seconds: Option<u32>,
    /// Whether the declaration asked for open egress.
    ///
    /// Carried because an empty connector list cannot otherwise be read: a MicroVM started with
    /// no connector reaches the internet, so a `deny` binding stripped of its connectors would be
    /// indistinguishable from `allow`. Absent means `deny`, which is the answer that fails closed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_egress: bool,
}

/// Azure sandbox binding configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AzureSandboxBinding {
    /// Default runtime policy when create omits one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_policy: Option<EgressPolicy>,
    /// Sandbox group that scopes every sandbox, image, snapshot and secret
    pub sandbox_group: BindingValue<String>,
    /// ADC data-plane endpoint, which is separate from the ARM control plane
    pub data_plane_endpoint: BindingValue<String>,
    /// Region the sandbox group lives in; selects the per-region ADC endpoint
    pub region: BindingValue<String>,
    /// Resource group the sandbox group sits in. The data-plane path is scoped by it, and the
    /// Azure client config does not carry one.
    pub resource_group: BindingValue<String>,
    /// Outbound policy every sandbox is created with, as declared.
    ///
    /// Carried whole rather than as a flag: the data plane's default action is `Allow`, so a
    /// sandbox created without a policy is an open one, and a hostname list has no boolean to
    /// travel in.
    pub egress: SandboxEgressBinding,
    /// Idle seconds after which a sandbox pauses, if the declaration asked for one.
    ///
    /// Carried because the data plane takes it at create and nowhere else: a policy that does not
    /// travel with the create body is a declaration the sandbox never hears about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_pause_seconds: Option<u32>,
    /// Catalog name or registry image every sandbox is created from, as `code` declares it. A
    /// registry image is resolved by label to the disk image the controller built, so the value
    /// stays one a setup package can render.
    pub disk_image: BindingValue<String>,
    /// Sandbox ceilings in the data plane's own units, from the declaration. Optional because a
    /// binding from an earlier release carries none — a required field would fail to deserialize
    /// on an already-running deployment. Absent takes the data plane's own default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu: Option<BindingValue<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<BindingValue<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk: Option<BindingValue<String>>,
}

/// GCP Agent Platform sandbox binding configuration.
///
/// Sandboxes have a durable parent to address: an Agent Engine provisioned at deploy and reached
/// through a regional endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GcpAgentPlatformSandboxBinding {
    /// Default runtime policy when create omits one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_policy: Option<EgressPolicy>,
    /// Declared internet access. Absent only in bindings created by older releases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub egress: Option<SandboxEgressBinding>,
    /// Agent Engine that parents every sandbox. Sandboxes are created and enumerated under it,
    /// so a binding without it can neither reach nor reap them.
    pub engine: BindingValue<String>,
    /// Template every sandbox is created from. It carries the image digest, the ceilings and the
    /// egress rules, so a sandbox created without it runs an unpinned image with none applied.
    pub template: BindingValue<String>,
    /// Region selecting the regional aiplatform endpoint. The engine is regional with no global
    /// alias, so the endpoint cannot be derived without it.
    pub region: BindingValue<String>,
    /// Seconds a sandbox may live, from the declaration. Carried only when one was declared; an
    /// absent value takes the service default, which is why it is not defaulted here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_lifetime_seconds: Option<u32>,
    /// Whether the declaration asked for open egress. The template enforces the policy; this lets a
    /// reader that never sees the template tell `allow` from `deny`. Absent means `deny`.
    #[serde(default)]
    pub allow_egress: bool,
}

/// Kubernetes sandbox binding configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesSandboxBinding {
    /// Default runtime policy when create omits one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_policy: Option<EgressPolicy>,
    /// Declared internet access. Absent only in bindings created by older releases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub egress: Option<SandboxEgressBinding>,
    /// Namespace sandbox pods are created in
    pub namespace: BindingValue<String>,
    /// Runtime class every sandbox pod must carry, such as `gvisor` or `kata`
    pub runtime_class: BindingValue<String>,
    /// Label selector identifying this sandbox's pods, used for enumeration and reaping
    pub selector: BindingValue<String>,
    /// Sandbox broker served by the operator. Claiming a pod is a `PATCH` on pods, which must
    /// not reach the application.
    pub broker_url: BindingValue<String>,
    /// Secret holding the capability signing key, by name. The binding names it; only the
    /// broker can read it.
    pub key_name: BindingValue<String>,
    /// Where Kubernetes mounted the pod's own ServiceAccount token. A path, not a secret: the
    /// platform put the file there and the broker verifies it with a `TokenReview`.
    pub token_path: BindingValue<String>,
}

/// Local sandbox binding configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSandboxBinding {
    /// Default runtime policy when create omits one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_policy: Option<EgressPolicy>,
    /// Declared internet access. Absent only in bindings created by older releases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub egress: Option<SandboxEgressBinding>,
    /// Loopback endpoint of the local sandbox manager
    pub manager_url: BindingValue<String>,
    /// Key scoping this binding's sandboxes within the manager
    pub sandbox_key: BindingValue<String>,
    /// File holding the route's bearer token. A locator, not the token: a binding is
    /// serialized into the workload's environment, and a secret there is a secret in state.
    pub token_path: BindingValue<String>,
}

impl SandboxBinding {
    /// Egress handoff, including declarations stored by earlier releases.
    pub fn egress(&self) -> Option<&SandboxEgressBinding> {
        match self {
            Self::Aws(value) => value.egress.as_ref(),
            Self::Azure(value) => Some(&value.egress),
            Self::GcpAgentPlatform(value) => value.egress.as_ref(),
            Self::Kubernetes(value) => value.egress.as_ref(),
            Self::Local(value) => value.egress.as_ref(),
        }
    }
    /// Default runtime policy, including the released allowDomains alias.
    pub fn default_policy(&self) -> Option<&EgressPolicy> {
        let explicit = match self {
            Self::Aws(value) => value.default_policy.as_ref(),
            Self::Azure(value) => value.default_policy.as_ref(),
            Self::GcpAgentPlatform(value) => value.default_policy.as_ref(),
            Self::Kubernetes(value) => value.default_policy.as_ref(),
            Self::Local(value) => value.default_policy.as_ref(),
        };
        explicit.or_else(|| self.egress().and_then(SandboxEgressBinding::default_policy))
    }

    /// Creates an AWS sandbox binding.
    pub fn aws(
        image_arn: impl Into<BindingValue<String>>,
        image_version: impl Into<BindingValue<String>>,
        region: impl Into<BindingValue<String>>,
    ) -> Self {
        Self::Aws(AwsSandboxBinding {
            default_policy: None,
            egress: None,
            image_arn: image_arn.into(),
            image_version: image_version.into(),
            region: region.into(),
            execution_role_arn: None,
            egress_connector_arns: Vec::new(),
            preview_ports: Vec::new(),
            idle_pause_seconds: None,
            max_lifetime_seconds: None,
            allow_egress: false,
        })
    }

    /// Creates an Azure sandbox binding.
    pub fn azure(
        sandbox_group: impl Into<BindingValue<String>>,
        data_plane_endpoint: impl Into<BindingValue<String>>,
        region: impl Into<BindingValue<String>>,
        resource_group: impl Into<BindingValue<String>>,
        disk_image: impl Into<BindingValue<String>>,
        egress: SandboxEgress,
        idle_pause_seconds: Option<u32>,
    ) -> Self {
        Self::Azure(AzureSandboxBinding {
            default_policy: None,
            sandbox_group: sandbox_group.into(),
            data_plane_endpoint: data_plane_endpoint.into(),
            region: region.into(),
            resource_group: resource_group.into(),
            egress: egress.into(),
            idle_pause_seconds,
            disk_image: disk_image.into(),
            // Ceilings are set on the struct where a caller has them; a positional argument each
            // would make this constructor ten wide for the case that rarely carries them.
            cpu: None,
            memory: None,
            disk: None,
        })
    }

    /// Creates a GCP Agent Platform sandbox binding.
    pub fn gcp_agent_platform(
        engine: impl Into<BindingValue<String>>,
        template: impl Into<BindingValue<String>>,
        region: impl Into<BindingValue<String>>,
        max_lifetime_seconds: Option<u32>,
        allow_egress: bool,
    ) -> Self {
        Self::GcpAgentPlatform(GcpAgentPlatformSandboxBinding {
            default_policy: None,
            egress: Some(
                (if allow_egress {
                    SandboxEgress::Internet
                } else {
                    SandboxEgress::None
                })
                .into(),
            ),
            engine: engine.into(),
            template: template.into(),
            region: region.into(),
            max_lifetime_seconds,
            allow_egress,
        })
    }

    /// Creates a Kubernetes sandbox binding.
    pub fn kubernetes(
        namespace: impl Into<BindingValue<String>>,
        runtime_class: impl Into<BindingValue<String>>,
        selector: impl Into<BindingValue<String>>,
        broker_url: impl Into<BindingValue<String>>,
        key_name: impl Into<BindingValue<String>>,
        token_path: impl Into<BindingValue<String>>,
        egress: Option<SandboxEgress>,
    ) -> Self {
        Self::Kubernetes(KubernetesSandboxBinding {
            default_policy: None,
            egress: egress.map(Into::into),
            namespace: namespace.into(),
            runtime_class: runtime_class.into(),
            selector: selector.into(),
            broker_url: broker_url.into(),
            key_name: key_name.into(),
            token_path: token_path.into(),
        })
    }

    /// Creates a local sandbox binding.
    pub fn local(
        manager_url: impl Into<BindingValue<String>>,
        sandbox_key: impl Into<BindingValue<String>>,
        token_path: impl Into<BindingValue<String>>,
        egress: Option<SandboxEgress>,
    ) -> Self {
        Self::Local(LocalSandboxBinding {
            default_policy: None,
            egress: egress.map(Into::into),
            manager_url: manager_url.into(),
            sandbox_key: sandbox_key.into(),
            token_path: token_path.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::{ContainerBinding, KvBinding};

    #[test]
    fn every_variant_roundtrips() {
        let bindings = vec![
            SandboxBinding::aws(
                "arn:aws:lambda:us-east-2:1:microvm-image:sbx",
                "3",
                "us-east-2",
            ),
            SandboxBinding::azure(
                "sbg1",
                "https://management.swedencentral.azuredevcompute.io",
                "swedencentral",
                "rg",
                "ubuntu",
                SandboxEgress::None,
                None,
            ),
            SandboxBinding::gcp_agent_platform(
                "projects/p/locations/us-central1/reasoningEngines/1",
                "projects/p/locations/us-central1/sandboxTemplates/agent",
                "us-central1",
                Some(3600),
                true,
            ),
            SandboxBinding::kubernetes(
                "alien-sandboxes",
                "gvisor",
                "alien.dev/sandbox=agent",
                "http://alien-operator.alien.svc:8080",
                "alien-sandbox-agent-capability",
                "/var/run/secrets/kubernetes.io/serviceaccount/token",
                None,
            ),
            SandboxBinding::local(
                "http://127.0.0.1:8931",
                "agent",
                "/state/sandbox-manager.token",
                None,
            ),
        ];

        for binding in bindings {
            let json = serde_json::to_string(&binding).expect("serializes");
            let restored: SandboxBinding = serde_json::from_str(&json).expect("deserializes");
            assert_eq!(binding, restored, "roundtrip changed the binding: {json}");
        }
    }

    /// The Agent Platform binding carries an egress-bearing template, so there is no safe default
    /// for a missing required field: a binding stripped of one must fail to load rather than
    /// deserialize into a sandbox with no image, no limits and open egress. `maxLifetimeSeconds` is
    /// the one field that may be absent, and its absence must still parse.
    #[test]
    fn agent_platform_required_fields_have_no_default() {
        let binding = SandboxBinding::gcp_agent_platform(
            "projects/p/locations/us-central1/reasoningEngines/1",
            "projects/p/locations/us-central1/sandboxTemplates/agent",
            "us-central1",
            Some(3600),
            true,
        );
        let full = serde_json::to_value(&binding).expect("serializes");

        for required in ["engine", "template", "region"] {
            let mut stripped = full.clone();
            stripped
                .as_object_mut()
                .expect("binding serializes as an object")
                .remove(required)
                .expect("the field is present before it is stripped");
            serde_json::from_value::<SandboxBinding>(stripped)
                .expect_err(&format!("a binding missing '{required}' must not load"));
        }

        let mut without_ttl = full;
        without_ttl
            .as_object_mut()
            .expect("binding serializes as an object")
            .remove("maxLifetimeSeconds")
            .expect("the fixture set a ttl");
        without_ttl
            .as_object_mut()
            .expect("binding serializes as an object")
            .remove("allowEgress")
            .expect("the fixture allowed egress");
        without_ttl.as_object_mut().unwrap().remove("egress");
        let restored: SandboxBinding = serde_json::from_value(without_ttl)
            .expect("legacy binding without optional fields loads");
        let SandboxBinding::GcpAgentPlatform(restored) = restored else {
            panic!("the service tag must select Agent Platform");
        };
        assert_eq!(restored.max_lifetime_seconds, None);
        assert!(!restored.allow_egress, "legacy bindings default to deny");
        assert_eq!(
            restored.egress, None,
            "legacy bindings carry no explicit declaration"
        );
    }

    #[test]
    fn service_tags_are_prefixed_and_distinct() {
        let tags: Vec<String> = vec![
            SandboxBinding::aws("a", "1", "r"),
            SandboxBinding::azure("g", "e", "r", "rg", "ubuntu", SandboxEgress::None, None),
            SandboxBinding::gcp_agent_platform("e", "t", "us-central1", None, false),
            SandboxBinding::kubernetes("n", "gvisor", "s", "http://op:8080", "k", "/t", None),
            SandboxBinding::local("u", "k", "t", None),
        ]
        .iter()
        .map(|binding| {
            serde_json::to_value(binding).expect("serializes")["service"]
                .as_str()
                .expect("has a service tag")
                .to_string()
        })
        .collect();

        for tag in &tags {
            assert!(tag.starts_with("sandbox-"), "tag '{tag}' is not namespaced");
        }

        let mut unique = tags.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), tags.len(), "duplicate service tags: {tags:?}");
    }

    /// The failure the bindings AGENTS.md warns about: with `tag = "service"`, serde picks the
    /// variant on that field alone, so a shared tag lets one resource's binding deserialize as
    /// another's by dropping the fields that differ.
    #[test]
    fn a_sandbox_binding_cannot_deserialize_as_another_resource() {
        let json = serde_json::to_string(&SandboxBinding::local(
            "http://127.0.0.1:8931",
            "agent",
            "/state/sandbox-manager.token",
            None,
        ))
        .expect("serializes");

        serde_json::from_str::<KvBinding>(&json)
            .expect_err("a sandbox binding must not parse as a KV binding");
        serde_json::from_str::<ContainerBinding>(&json)
            .expect_err("a sandbox binding must not parse as a container binding");
    }

    #[test]
    fn another_resource_binding_cannot_deserialize_as_a_sandbox() {
        let json = serde_json::to_string(&ContainerBinding::local("api", "http://api.svc:8080"))
            .expect("serializes");

        serde_json::from_str::<SandboxBinding>(&json)
            .expect_err("a container binding must not parse as a sandbox binding");
    }
}
