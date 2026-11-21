//! The GCP wiring that sends every Internet sandbox's outbound traffic through the egress gateway.
//!
//! Agent Platform connects a sandbox to the deployment's VPC through a Private Service Connect
//! interface: a network attachment on a dedicated subnet. Traffic from the sandbox enters the VPC
//! from that subnet, and the firewall lets it reach the gateway's proxy and nothing else.
//!
//! Setup creates the wiring twice, as Terraform and as direct API calls. Both render from the
//! description in this file (names, ranges, ports, role permissions and firewall rules), and a
//! differential test compares the two, so the two setup surfaces cannot drift apart.

/// The one switch for Google Cloud internet egress through the gateway. While it is `false`,
/// every plan-time path (build, release, the Terraform and CloudFormation render, deployment
/// preparation) refuses a Google Cloud sandbox with egress `internet`, so no setup creates the
/// network attachment, the gateway VMs or Private Service Connect resources, and no template
/// pins a project to Private Service Connect. Flipping it to `true` is the whole release step,
/// once the on-demand live test (`alien-test`, `gcp_sandbox_internet_live`) passes.
pub const INTERNET_EGRESS_AVAILABLE: bool = false;

/// The refusal while [`INTERNET_EGRESS_AVAILABLE`] is off. Build, render and deployment checks
/// all give this text.
pub const INTERNET_SANDBOX_UNAVAILABLE: &str = "Google Cloud internet egress through the egress \
     gateway is not available yet. Use egress none, or deploy the sandbox on another cloud.";

/// Said at plan time for a Google Cloud sandbox on Google's default network. The first sandbox a
/// project runs fixes its networking for good, so a project that started on the default network
/// can never host a gateway-backed `internet` sandbox.
pub const DEFAULT_PLACEMENT_IS_PERMANENT: &str = "this sandbox uses Google's default network. \
     A Google Cloud project whose first sandbox used the default network can never run sandboxes \
     with egress `internet` through the deployment's egress gateway, even after every sandbox is \
     deleted. To use egress `internet` later, deploy to a new project.";

/// Whether Google Cloud sandboxes may declare egress `internet`: the switch, or a test that
/// turned it on for its own thread or its whole process.
pub fn internet_egress_available() -> bool {
    INTERNET_EGRESS_AVAILABLE || test_switch::forced()
}

#[cfg(any(test, feature = "test-gcp-internet-egress"))]
mod test_switch {
    use std::cell::Cell;
    use std::sync::atomic::{AtomicBool, Ordering};

    thread_local! {
        static FORCED: Cell<bool> = const { Cell::new(false) };
    }

    static FORCED_FOR_PROCESS: AtomicBool = AtomicBool::new(false);

    pub(super) fn forced() -> bool {
        FORCED.with(Cell::get) || FORCED_FOR_PROCESS.load(Ordering::SeqCst)
    }

    /// Turns Google Cloud internet egress on for every thread of this process, for good: the
    /// live test, whose build, manager and controllers run on the runtime's worker threads, plans
    /// and deploys the stack the flipped release would. Never used by unit tests, which share a
    /// process and must see the switch off.
    pub fn enable_internet_egress_for_process() {
        FORCED_FOR_PROCESS.store(true, Ordering::SeqCst);
    }

    /// Keeps the switch on for the current thread until it is dropped.
    #[must_use = "the switch turns off again when the guard is dropped"]
    pub struct InternetEgressForTest(());

    impl Drop for InternetEgressForTest {
        fn drop(&mut self) {
            FORCED.with(|forced| forced.set(false));
        }
    }

    /// Turns Google Cloud internet egress on for the current thread, as the flipped switch
    /// would. Tests only; production code never reaches this.
    pub fn enable_internet_egress_for_test() -> InternetEgressForTest {
        FORCED.with(|forced| forced.set(true));
        InternetEgressForTest(())
    }
}

#[cfg(not(any(test, feature = "test-gcp-internet-egress")))]
mod test_switch {
    pub(super) fn forced() -> bool {
        false
    }
}

#[cfg(any(test, feature = "test-gcp-internet-egress"))]
pub use test_switch::{
    enable_internet_egress_for_process, enable_internet_egress_for_test, InternetEgressForTest,
};

/// Range of the attachment's own subnet. Sandbox traffic enters the VPC from addresses in it. A
/// regular subnet of at least /28 that is not 100.64.0.0/20 or 240.0.0.0/4.
pub const ATTACHMENT_CIDR: &str = "192.168.250.0/28";

/// Range of the gateway subnet, the only subnet with Cloud NAT.
pub const GATEWAY_CIDR: &str = "192.168.251.0/27";

/// Host numbers of the two gateway VMs inside the gateway subnet (`.10` and `.11`). GCP reserves
/// the first two and last two addresses of a subnet.
pub const GATEWAY_HOSTS: [u8; 2] = [10, 11];

/// The port the gateway container's proxy listens on.
pub const CONTAINER_PROXY_PORT: u16 = 3128;

/// The port of the gateway's authenticated policy administration. Never reachable from the
/// attachment range.
pub const ADMIN_PORT: u16 = 3129;

/// The port the sandboxes connect to on the gateway VMs, the one setting that says which port
/// crosses from Agent Platform's tenant into the VPC. Only tcp/22 does today (measured, not
/// documented by Google). Every place that needs the port reads it from the setup-recorded
/// network data, which setup writes from this default.
pub const DEFAULT_PROXY_PORT: u16 = 22;

/// The network tag the gateway VMs carry and the firewall rules target.
pub const GATEWAY_TAG: &str = "alien-sandbox-gateway";

/// Permissions the Agent Platform service agent needs on the attachment. It must hold them, and
/// they must have propagated, before the first template names the attachment: a template that
/// fails on them leaves that attachment unusable for good.
pub const ATTACHMENT_ROLE_PERMISSIONS: [&str; 2] = [
    "compute.networkAttachments.get",
    "compute.networkAttachments.update",
];

/// Seconds setup waits after binding the role, before anything creates a template. Three minutes
/// worked in a measurement and ninety seconds did not.
pub const ROLE_PROPAGATION_SECONDS: u64 = 180;

/// A name the wiring gives a resource: `{resource prefix}-{suffix}`, at most 63 characters with no
/// leading or trailing dash. Terraform computes the same string in HCL.
pub fn resource_name(resource_prefix: &str, suffix: &str) -> String {
    let full = format!("{resource_prefix}-{suffix}");
    full.chars()
        .take(63)
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Custom role ids allow letters, digits, `_` and `.`, at most 64 characters.
pub fn role_id(resource_prefix: &str) -> String {
    let full = format!("{resource_prefix}_sandbox_attachment");
    full.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect()
}

/// The gateway VMs' machine type unless the setup says otherwise: at least 2 vCPUs and 4 GiB.
pub const DEFAULT_MACHINE_TYPE: &str = "e2-standard-2";

/// The gateway VMs' boot image.
pub const BOOT_IMAGE: &str = "projects/ubuntu-os-cloud/global/images/family/ubuntu-2404-lts-amd64";

/// The gateway VMs' boot disk size.
pub const BOOT_DISK_GB: u32 = 20;

/// Account id of the gateways' identity: `sbx-gw-` and the prefix, at most 30 characters and
/// never ending in a dash. The Terraform emitter renders the same with `trim(substr(..))`.
pub fn service_account_id(resource_prefix: &str) -> String {
    format!("sbx-gw-{resource_prefix}")
        .chars()
        .take(30)
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// The condition that limits the gateways' secret access to the given secrets of the deployment
/// vault. `secrets` are the secret names without the vault prefix.
pub fn secret_condition(project_number: &str, vault_prefix: &str, secrets: &[&str]) -> String {
    secrets
        .iter()
        .map(|secret| {
            format!(
                "resource.name.startsWith(\"projects/{project_number}/secrets/{vault_prefix}-{secret}/\")"
            )
        })
        .collect::<Vec<_>>()
        .join(" || ")
}

/// Name suffixes of every resource of the wiring.
pub mod suffix {
    pub const ATTACHMENT_SUBNET: &str = "sbx-attachment";
    pub const ATTACHMENT: &str = "sbx-attachment";
    pub const GATEWAY_SUBNET: &str = "sbx-gateway";
    pub const ROUTER: &str = "sbx-gateway-router";
    pub const NAT: &str = "sbx-gateway-nat";
    pub const GATEWAY_SERVICE_ACCOUNT: &str = "sandbox_gateway";
    pub const FIREWALL_PROXY: &str = "sbx-allow-proxy";
    pub const FIREWALL_ADMIN: &str = "sbx-allow-admin";
    pub const FIREWALL_DENY: &str = "sbx-deny-attachment";

    /// Name suffix of gateway VM `index`.
    pub fn vm(index: usize) -> String {
        format!("sbx-gateway-{index}")
    }
}

/// Where a firewall rule's traffic comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleSource {
    /// The attachment subnet: sandbox traffic.
    Attachment,
    /// The gateway subnet: the peer gateway.
    Gateway,
    /// The deployment's workload subnet: the manager's path to the admin port.
    Workload,
}

/// One firewall rule of the wiring. Every rule is ingress to the gateway VMs (the tag) except the
/// catch-all deny, which covers the whole VPC from the attachment range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirewallRule {
    pub suffix: &'static str,
    pub priority: u32,
    pub allow: bool,
    pub sources: Vec<RuleSource>,
    /// `None` is every protocol and port.
    pub tcp_ports: Option<Vec<u16>>,
    /// Targets only the gateway VMs. A rule that does not covers the whole network.
    pub gateway_only: bool,
    pub log: bool,
}

/// The firewall rules, in priority order. `proxy_port` is the port sandboxes connect to.
///
/// - the attachment subnet reaches the gateways on `proxy_port` and nothing else;
/// - the admin port answers only the peer gateway and the deployment's workloads;
/// - everything else from the attachment subnet is denied and logged, in the whole VPC.
///
/// The VPC denies ingress by default. The deny rule exists to log, and to win over any broad allow
/// a customer's network carries at a lower priority.
pub fn firewall_rules(proxy_port: u16) -> Vec<FirewallRule> {
    vec![
        FirewallRule {
            suffix: suffix::FIREWALL_PROXY,
            priority: 900,
            allow: true,
            sources: vec![RuleSource::Attachment],
            tcp_ports: Some(vec![proxy_port]),
            gateway_only: true,
            log: true,
        },
        FirewallRule {
            suffix: suffix::FIREWALL_ADMIN,
            priority: 900,
            allow: true,
            sources: vec![RuleSource::Gateway, RuleSource::Workload],
            tcp_ports: Some(vec![ADMIN_PORT]),
            gateway_only: true,
            log: false,
        },
        FirewallRule {
            suffix: suffix::FIREWALL_DENY,
            priority: 1000,
            allow: false,
            sources: vec![RuleSource::Attachment],
            tcp_ports: None,
            gateway_only: false,
            log: true,
        },
    ]
}

/// The gateway VMs' private addresses, from the gateway range.
pub fn gateway_ips() -> Vec<String> {
    let base = GATEWAY_CIDR
        .split('/')
        .next()
        .expect("a CIDR has an address");
    let prefix: Vec<&str> = base.split('.').take(3).collect();
    GATEWAY_HOSTS
        .iter()
        .map(|host| format!("{}.{host}", prefix.join(".")))
        .collect()
}

/// Full path of the network attachment a template names.
pub fn attachment_path(project: &str, region: &str, name: &str) -> String {
    format!("projects/{project}/regions/{region}/networkAttachments/{name}")
}

/// Regions where Agent Platform accepts a sandbox template with a network attachment and private
/// ingress. Measured by creating one in each region: us-central1 accepts it, and us-east1,
/// us-east4, us-west1, europe-west1, europe-west2, europe-west4 and asia-southeast1 answer "not yet
/// available in region". Google lists no regions for it, so this is an allowlist: a region joins
/// it when a template there is accepted. A sandbox that declares `internet` anywhere else is
/// refused at plan time. It never falls back to open outbound.
pub const REGIONS_WITH_PRIVATE_SERVICE_CONNECT: &[&str] = &["us-central1"];

/// Why `internet` cannot be hosted in `region`, or `None` when it can.
pub fn region_refusal(region: &str) -> Option<String> {
    if REGIONS_WITH_PRIVATE_SERVICE_CONNECT.contains(&region) {
        return None;
    }
    Some(format!(
        "egress internet on GCP sends sandbox traffic through a Private Service Connect \
         interface, which Agent Platform does not offer in {region}; use {}",
        REGIONS_WITH_PRIVATE_SERVICE_CONNECT.join(" or ")
    ))
}

/// The refusal for a project whose sandbox placement is already pinned to the default network.
pub const PLACEMENT_PINNED_MESSAGE: &str = "egress internet on GCP needs a project that has \
never run a sandbox without the deployment network: Google pins a project's sandbox placement \
with its first sandbox and cannot change it. Deploy to a new project, or use egress none";

/// The gateway VM's boot script with three `%s` values, in order: the project id, the region and
/// the JSON of the secrets vault binding. Terraform fills them with `format`, direct setup with
/// [`startup_script`]. It installs Docker, writes the environment file and a unit that redirects
/// the proxy port from the attachment range to the container's own port (limited to that range,
/// so no other source reaches the proxy through it) and runs the gateway as one hardened,
/// unprivileged container. The container is unchanged: it listens on 3128.
pub fn startup_script_template(image: &str) -> String {
    let redirect = if DEFAULT_PROXY_PORT == CONTAINER_PROXY_PORT {
        String::new()
    } else {
        format!(
            "ExecStartPre=/bin/sh -c 'iptables -t nat -C PREROUTING -s {cidr} -p tcp --dport {port} -j REDIRECT --to-ports {to} || iptables -t nat -I PREROUTING 1 -s {cidr} -p tcp --dport {port} -j REDIRECT --to-ports {to}'",
            cidr = ATTACHMENT_CIDR,
            port = DEFAULT_PROXY_PORT,
            to = CONTAINER_PROXY_PORT,
        )
    };
    let lines = [
        "#!/bin/bash".to_string(),
        "set -u".to_string(),
        "for i in $(seq 1 30); do apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y docker.io iptables && break; sleep 10; done".to_string(),
        format!("for i in $(seq 1 30); do docker pull {image} && break; sleep 10; done"),
        "umask 077".to_string(),
        "cat > /etc/alien-egress-gateway.env <<'ENV'".to_string(),
        "ALIEN_DEPLOYMENT_TYPE=gcp".to_string(),
        "GCP_PROJECT_ID=%s".to_string(),
        "GCP_REGION=%s".to_string(),
        "ALIEN_SECRETS_BINDING=%s".to_string(),
        "RUST_LOG=warn".to_string(),
        "ENV".to_string(),
        "cat > /etc/systemd/system/alien-egress-gateway.service <<'UNIT'".to_string(),
        "[Unit]".to_string(),
        "Description=Alien egress gateway".to_string(),
        "After=docker.service network-online.target".to_string(),
        "Requires=docker.service".to_string(),
        "[Service]".to_string(),
        "Restart=always".to_string(),
        "RestartSec=3".to_string(),
        "ExecStartPre=-/usr/bin/docker rm -f gateway".to_string(),
        redirect,
        format!("ExecStart=/usr/bin/docker run --name gateway --env-file /etc/alien-egress-gateway.env --user 1000:1000 --cap-drop ALL --memory 2g --security-opt no-new-privileges --read-only --tmpfs /tmp -p {CONTAINER_PROXY_PORT}:{CONTAINER_PROXY_PORT} -p {ADMIN_PORT}:{ADMIN_PORT} {image}"),
        "[Install]".to_string(),
        "WantedBy=multi-user.target".to_string(),
        "UNIT".to_string(),
        "systemctl daemon-reload".to_string(),
        "systemctl enable alien-egress-gateway.service".to_string(),
        "systemctl restart alien-egress-gateway.service".to_string(),
    ];
    let mut text = lines
        .into_iter()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    text.push('\n');
    text
}

/// [`startup_script_template`] with its values filled for direct setup.
pub fn startup_script(image: &str, project: &str, region: &str, vault_prefix: &str) -> String {
    let binding = serde_json::json!({"service": "secret-manager", "vaultPrefix": vault_prefix});
    let mut values = [project.to_string(), region.to_string(), binding.to_string()].into_iter();
    let mut out = String::new();
    let mut rest = startup_script_template(image);
    while let Some(index) = rest.find("%s") {
        out.push_str(&rest[..index]);
        out.push_str(&values.next().expect("three values"));
        rest = rest[index + 2..].to_string();
    }
    out.push_str(&rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The switch ships off; a test can turn it on for its own thread only, and the guard
    /// turns it off again.
    #[test]
    fn internet_egress_is_off_until_the_switch_flips() {
        assert!(!INTERNET_EGRESS_AVAILABLE);
        assert!(!internet_egress_available());
        {
            let _on = enable_internet_egress_for_test();
            assert!(internet_egress_available());
            std::thread::spawn(|| assert!(!internet_egress_available()))
                .join()
                .unwrap();
        }
        assert!(!internet_egress_available());
    }

    #[test]
    fn the_boot_script_fills_its_three_values_and_names_the_port_from_the_constant() {
        let text = startup_script(
            "registry.example/gw@sha256:abc",
            "proj",
            "us-central1",
            "p-secrets",
        );
        assert!(!text.contains("%s"), "{text}");
        assert!(text.contains("GCP_PROJECT_ID=proj") && text.contains("GCP_REGION=us-central1"));
        assert!(
            text.contains(r#"{"service":"secret-manager","vaultPrefix":"p-secrets"}"#),
            "{text}"
        );
        assert!(text.contains(&format!("--dport {DEFAULT_PROXY_PORT}")));
        assert_eq!(startup_script_template("i").matches("%s").count(), 3);
    }

    #[test]
    fn the_service_account_id_fits_the_limit() {
        assert_eq!(service_account_id("acme"), "sbx-gw-acme");
        let long = service_account_id(&format!("{}-", "a".repeat(40)));
        assert!(long.len() <= 30 && !long.ends_with('-'), "{long}");
    }

    #[test]
    fn the_secret_condition_names_each_secret_path() {
        let text = secret_condition("123", "p-secrets", &["a", "b"]);
        assert_eq!(
            text,
            r#"resource.name.startsWith("projects/123/secrets/p-secrets-a/") || resource.name.startsWith("projects/123/secrets/p-secrets-b/")"#
        );
    }

    #[test]
    fn names_are_clamped_and_never_end_in_a_dash() {
        let name = resource_name(&"a".repeat(70), "sbx-attachment");
        assert!(name.len() <= 63);
        assert!(!name.ends_with('-'));
        assert_eq!(resource_name("acme", "sbx-gateway-0"), "acme-sbx-gateway-0");
    }

    #[test]
    fn the_gateway_addresses_sit_in_the_gateway_range() {
        assert_eq!(gateway_ips(), vec!["192.168.251.10", "192.168.251.11"]);
    }

    #[test]
    fn only_the_proxy_port_is_open_to_the_attachment() {
        for port in [DEFAULT_PROXY_PORT, 2222] {
            let rules = firewall_rules(port);
            for rule in rules.iter().filter(|r| r.allow) {
                if rule.sources.contains(&RuleSource::Attachment) {
                    assert_eq!(rule.tcp_ports, Some(vec![port]));
                    assert!(rule.gateway_only);
                }
            }
            let deny = rules.iter().find(|r| !r.allow).unwrap();
            assert!(
                deny.log
                    && deny.priority
                        > rules
                            .iter()
                            .filter(|r| r.allow)
                            .map(|r| r.priority)
                            .max()
                            .unwrap()
            );
            assert!(!rules
                .iter()
                .any(|r| r.sources.contains(&RuleSource::Attachment)
                    && r.tcp_ports == Some(vec![ADMIN_PORT])));
        }
    }

    #[test]
    fn a_region_without_private_service_connect_is_refused_by_name() {
        for region in ["us-east4", "europe-west4", "asia-southeast1"] {
            let text = region_refusal(region).expect("refused");
            assert!(text.contains(region) && text.contains("us-central1"));
        }
        assert!(region_refusal("us-central1").is_none());
    }
}
