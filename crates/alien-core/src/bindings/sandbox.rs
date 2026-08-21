//! Sandbox binding definitions.
//!
//! Carries what a provider needs to reach the durable parent and create sessions inside it.
//! Session identity is never in here — sessions are created at runtime and the provider is the
//! record, so a binding describes the parent only.

use super::BindingValue;
use crate::SandboxEgress;
use serde::{Deserialize, Serialize};

/// Represents a sandbox binding for creating and reaching sandbox sessions.
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
    /// Cloud Run sandboxes, launched inside the workload's own instance
    #[serde(rename = "sandbox-gcp")]
    Gcp(GcpSandboxBinding),
    /// Sandbox pods under a sandboxed runtime class
    #[serde(rename = "sandbox-kubernetes")]
    Kubernetes(KubernetesSandboxBinding),
    /// Local Docker sandboxes managed by the local sandbox manager
    #[serde(rename = "sandbox-local")]
    Local(LocalSandboxBinding),
}

/// AWS sandbox binding configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsSandboxBinding {
    /// MicroVM image ARN that scopes this sandbox's sessions
    pub image_arn: BindingValue<String>,
    /// Image version. Sessions are enumerated by image and version together, so a rolled
    /// version remains a cleanup scope until its own MicroVMs are gone.
    pub image_version: BindingValue<String>,
    /// Region the MicroVMs run in
    pub region: BindingValue<String>,
    /// Execution role attached to each MicroVM, distinct from the workload's own role
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_role_arn: Option<BindingValue<String>>,
    /// Egress connectors every session is started with.
    ///
    /// Carried rather than implied: a MicroVM started with no connector reaches the public
    /// internet, so an empty list here is `allow`, not `deny`. The declared mode is realised by
    /// which connector setup built, and the session has to be started with it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub egress_connector_arns: Vec<BindingValue<String>>,
    /// Ports a preview capability may be minted for.
    ///
    /// Carried because the token is what grants ingress: `CreateMicrovmAuthToken` mints access to
    /// whatever port it is asked for, so "a port not listed here can never be exposed" is only
    /// true if the declared list reaches the code that mints.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preview_ports: Vec<u16>,
    /// Idle seconds after which a session suspends, if the declaration asked for one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_suspend_seconds: Option<u32>,
    /// Wall-clock ceiling on a session, if the declaration asked for one.
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
    /// Sandbox group that scopes every sandbox, image, snapshot and secret
    pub sandbox_group: BindingValue<String>,
    /// ADC data-plane endpoint, which is separate from the ARM control plane
    pub data_plane_endpoint: BindingValue<String>,
    /// Region the sandbox group lives in; selects the per-region ADC endpoint
    pub region: BindingValue<String>,
    /// Resource group the sandbox group sits in. The data-plane path is scoped by it, and the
    /// Azure client config does not carry one.
    pub resource_group: BindingValue<String>,
    /// Outbound policy every session is created with, as declared.
    ///
    /// Carried whole rather than as a flag: the data plane's default action is `Allow`, so a
    /// session created without a policy is an open one, and a hostname list has no boolean to
    /// travel in.
    pub egress: SandboxEgress,
    /// Catalog disk image every session is created from, taken from the declaration's `code`.
    ///
    /// Carried rather than hardcoded in the provider because the declaration is the only place
    /// that knows it, and a sandbox running an image its author did not choose is the one Azure
    /// gap that fails without an error.
    pub disk_image: BindingValue<String>,
}

/// GCP sandbox binding configuration.
///
/// There is no durable parent to address: a Cloud Run sandbox is a subprocess of the workload's
/// own instance, created through a CLI on the container's filesystem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GcpSandboxBinding {
    /// Path to the sandbox CLI inside the Cloud Run container
    pub launcher_path: BindingValue<String>,
    /// Whether sandboxes may reach the network. Carried in the binding rather than passed per
    /// create: the launcher takes `--allow-egress` per sandbox, and a limit the application
    /// supplies is a limit it can decline to supply.
    pub allow_egress: BindingValue<bool>,
}

/// Kubernetes sandbox binding configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesSandboxBinding {
    /// Namespace sandbox pods are created in
    pub namespace: BindingValue<String>,
    /// Runtime class every sandbox pod must carry, such as `gvisor` or `kata`
    pub runtime_class: BindingValue<String>,
    /// Label selector identifying this sandbox's pods, used for enumeration and reaping
    pub selector: BindingValue<String>,
    /// Session broker served by the operator. Claiming a pod is a `PATCH` on pods, which must
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
    /// Loopback endpoint of the local sandbox manager
    pub manager_url: BindingValue<String>,
    /// Key scoping this sandbox's sessions within the manager
    pub sandbox_key: BindingValue<String>,
    /// File holding the route's bearer token. A locator, not the token: a binding is
    /// serialized into the workload's environment, and a secret there is a secret in state.
    pub token_path: BindingValue<String>,
}

impl SandboxBinding {
    /// Creates an AWS sandbox binding.
    pub fn aws(
        image_arn: impl Into<BindingValue<String>>,
        image_version: impl Into<BindingValue<String>>,
        region: impl Into<BindingValue<String>>,
    ) -> Self {
        Self::Aws(AwsSandboxBinding {
            image_arn: image_arn.into(),
            image_version: image_version.into(),
            region: region.into(),
            execution_role_arn: None,
            egress_connector_arns: Vec::new(),
            preview_ports: Vec::new(),
            idle_suspend_seconds: None,
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
    ) -> Self {
        Self::Azure(AzureSandboxBinding {
            sandbox_group: sandbox_group.into(),
            data_plane_endpoint: data_plane_endpoint.into(),
            region: region.into(),
            resource_group: resource_group.into(),
            egress,
            disk_image: disk_image.into(),
        })
    }

    /// Creates a GCP sandbox binding.
    pub fn gcp(
        launcher_path: impl Into<BindingValue<String>>,
        allow_egress: impl Into<BindingValue<bool>>,
    ) -> Self {
        Self::Gcp(GcpSandboxBinding {
            launcher_path: launcher_path.into(),
            allow_egress: allow_egress.into(),
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
    ) -> Self {
        Self::Kubernetes(KubernetesSandboxBinding {
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
    ) -> Self {
        Self::Local(LocalSandboxBinding {
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
                SandboxEgress::Deny,
            ),
            SandboxBinding::gcp("/usr/local/gcp/bin/sandbox", false),
            SandboxBinding::kubernetes(
                "alien-sandboxes",
                "gvisor",
                "alien.dev/sandbox=agent",
                "http://alien-operator.alien.svc:8080",
                "alien-sandbox-agent-capability",
                "/var/run/secrets/kubernetes.io/serviceaccount/token",
            ),
            SandboxBinding::local(
                "http://127.0.0.1:8931",
                "agent",
                "/state/sandbox-manager.token",
            ),
        ];

        for binding in bindings {
            let json = serde_json::to_string(&binding).expect("serializes");
            let restored: SandboxBinding = serde_json::from_str(&json).expect("deserializes");
            assert_eq!(binding, restored, "roundtrip changed the binding: {json}");
        }
    }

    #[test]
    fn service_tags_are_prefixed_and_distinct() {
        let tags: Vec<String> = vec![
            SandboxBinding::aws("a", "1", "r"),
            SandboxBinding::azure("g", "e", "r", "rg", "ubuntu", SandboxEgress::Deny),
            SandboxBinding::gcp("p", true),
            SandboxBinding::kubernetes("n", "gvisor", "s", "http://op:8080", "k", "/t"),
            SandboxBinding::local("u", "k", "t"),
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
