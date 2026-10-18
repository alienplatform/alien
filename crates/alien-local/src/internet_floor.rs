//! Host-owned filtering for restricted Docker containers. Install before start;
//! Docker's shared bridge and guest-root processes cannot remove these rules.
//! The restricted container has neither NET_ADMIN nor NET_RAW, so it cannot
//! change its assigned MAC or forge frames carrying another source address.

use std::collections::BTreeSet;
use std::process::Stdio;

use alien_error::{AlienError, Context, IntoAlienError};
use bollard::network::{InspectNetworkOptions, ListNetworksOptions};
use bollard::Docker;
use ipnet::Ipv4Net;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::{ErrorData, Result};

const OWNER: &str = "alien_local_owned";
const RESERVED: &[&str] = &[
    "0.0.0.0/8",
    "10.0.0.0/8",
    "100.64.0.0/10",
    "127.0.0.0/8",
    "169.254.0.0/16",
    "172.16.0.0/12",
    "192.0.0.0/24",
    "192.0.2.0/24",
    "192.168.0.0/16",
    "198.18.0.0/15",
    "198.51.100.0/24",
    "203.0.113.0/24",
    "224.0.0.0/4",
    "240.0.0.0/4",
    "168.63.129.16/32",
];

fn unavailable(message: impl Into<String>) -> ErrorData {
    ErrorData::LocalInternetEgressUnavailable {
        message: message.into(),
    }
}

/// Non-secret host firewall identity persisted alongside Docker metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InternetFloor {
    table: String,
    bridge: String,
    outbound_interfaces: BTreeSet<String>,
    pub source_mac: String,
    denied: BTreeSet<Ipv4Net>,
}

impl InternetFloor {
    pub async fn prepare(
        docker: &Docker,
        endpoint: &str,
        network_name: &str,
        owner: &str,
        additional_denied: &[String],
    ) -> Result<Self> {
        if !cfg!(target_os = "linux") || !endpoint.starts_with("unix://") {
            return Err(AlienError::new(unavailable(
                "Internet egress requires a Linux Docker engine on this host",
            )));
        }
        if !tokio::fs::read_to_string("/proc/sys/net/bridge/bridge-nf-call-iptables")
            .await
            .is_ok_and(|value| value.trim() == "1")
        {
            return Err(AlienError::new(unavailable(
                "Internet egress requires Linux bridge IPv4 filtering to be enabled",
            )));
        }
        // This checks CAP_NET_ADMIN and nft availability before creating a container.
        tables().await?;
        let network = docker
            .inspect_network(network_name, None::<InspectNetworkOptions<String>>)
            .await
            .into_alien_error()
            .context(unavailable("Cannot inspect the Docker bridge"))?;
        if network.driver.as_deref() != Some("bridge") || network.enable_ipv6 == Some(true) {
            return Err(AlienError::new(unavailable(
                "Internet egress requires an IPv4 Docker bridge",
            )));
        }
        let bridge = network
            .options
            .as_ref()
            .and_then(|options| options.get("com.docker.network.bridge.name"))
            .cloned()
            .or_else(|| {
                network
                    .id
                    .as_ref()
                    .filter(|id| id.len() >= 12)
                    .map(|id| format!("br-{}", &id[..12]))
            })
            .ok_or_else(|| AlienError::new(unavailable("Docker did not identify its bridge")))?;
        if bridge.len() > 15
            || !bridge
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            || !std::path::Path::new("/sys/class/net")
                .join(&bridge)
                .join("bridge")
                .is_dir()
        {
            return Err(AlienError::new(unavailable(
                "The selected Docker bridge is not on this host",
            )));
        }
        // Only the host's default uplinks are internet paths. A Docker bridge
        // created later must not become reachable merely because its subnet is public.
        let routes = tokio::fs::read_to_string("/proc/net/route")
            .await
            .into_alien_error()
            .context(unavailable(
                "Cannot discover the host's IPv4 default routes",
            ))?;
        let mut outbound_interfaces = BTreeSet::new();
        for route in routes.lines().skip(1) {
            let fields: Vec<_> = route.split_whitespace().collect();
            if fields.len() < 8
                || fields[1] != "00000000"
                || fields[7] != "00000000"
                || !u32::from_str_radix(fields[3], 16).is_ok_and(|flags| flags & 1 != 0)
            {
                continue;
            }
            let interface = fields[0];
            if interface.len() > 15
                || !interface
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
                || std::path::Path::new("/sys/class/net")
                    .join(interface)
                    .join("bridge")
                    .is_dir()
            {
                return Err(AlienError::new(unavailable(
                    "The host's default route must use an identifiable non-bridge interface",
                )));
            }
            outbound_interfaces.insert(interface.to_string());
        }
        if outbound_interfaces.is_empty() {
            return Err(AlienError::new(unavailable(
                "Internet egress requires an IPv4 default route on this host",
            )));
        }
        let mut denied = RESERVED
            .iter()
            .map(|cidr| cidr.parse().expect("reserved IPv4 CIDR"))
            .collect::<BTreeSet<Ipv4Net>>();
        for cidr in additional_denied {
            denied.insert(cidr.parse().map_err(|_| {
                AlienError::new(unavailable(
                    "An additional Docker egress exclusion is not an IPv4 CIDR",
                ))
            })?);
        }
        let networks = docker
            .list_networks(None::<ListNetworksOptions<String>>)
            .await
            .into_alien_error()
            .context(unavailable("Cannot discover Docker network address ranges"))?;
        for network in networks {
            for config in network
                .ipam
                .and_then(|ipam| ipam.config)
                .into_iter()
                .flatten()
            {
                if let Some(subnet) = config.subnet.and_then(|cidr| cidr.parse::<Ipv4Net>().ok()) {
                    denied.insert(subnet);
                }
            }
        }
        // Stable across process and compiler versions, unlike a runtime hash seed.
        let identity = format!("{:x}", Sha256::digest(owner.as_bytes()));
        let source_mac = format!(
            "02:{}:{}:{}:{}:{}",
            &identity[16..18],
            &identity[18..20],
            &identity[20..22],
            &identity[22..24],
            &identity[24..26]
        );
        Ok(Self {
            table: format!("alien_local_{}", &identity[..16]),
            bridge,
            outbound_interfaces,
            source_mac,
            denied,
        })
    }

    pub async fn install(&self) -> Result<()> {
        let existing = tables().await?;
        self.require_owned(&existing).await?;
        let mut rules = String::new();
        if table(&existing, &self.table).is_some() {
            rules.push_str(&format!("delete table inet {}\n", self.table));
        }
        let denied = self
            .denied
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        let outbound_interfaces = self
            .outbound_interfaces
            .iter()
            .map(|interface| format!("\"{interface}\""))
            .collect::<Vec<_>>()
            .join(", ");
        // A single nft transaction replaces the complete table without an open interval.
        rules.push_str(&format!(
            r#"table inet {table} {{
 chain {OWNER} {{ }}
 chain floor {{
  ct state established,related ct direction reply accept
  oifname != {{ {outbound_interfaces} }} drop
  ip daddr {{ {denied} }} drop
 }}
 chain incoming {{ type filter hook input priority -200; policy accept;
  iifname "{bridge}" ether saddr {source} ct state established,related ct direction reply accept
  iifname "{bridge}" ether saddr {source} drop
 }}
 chain forwarding {{ type filter hook forward priority -200; policy accept;
  iifname "{bridge}" ether saddr {source} jump floor
 }}
}}
"#,
            table = self.table,
            bridge = self.bridge,
            source = self.source_mac
        ));
        nft(&["-f", "-"], Some(&rules)).await.map(|_| ())
    }

    pub async fn remove(&self) -> Result<()> {
        let existing = tables().await?;
        self.require_owned(&existing).await?;
        if table(&existing, &self.table).is_some() {
            nft(&["delete", "table", "inet", &self.table], None).await?;
        }
        Ok(())
    }

    async fn require_owned(&self, tables: &serde_json::Value) -> Result<()> {
        if table(tables, &self.table).is_none() {
            return Ok(());
        }
        let bytes = nft(&["-j", "list", "table", "inet", &self.table], None).await?;
        let contents: serde_json::Value = serde_json::from_slice(&bytes)
            .into_alien_error()
            .context(unavailable(
                "Cannot inspect the existing host egress policy",
            ))?;
        let owned = contents["nftables"].as_array().is_some_and(|entries| {
            entries.iter().any(|entry| {
                entry
                    .get("chain")
                    .is_some_and(|chain| chain["name"] == OWNER && chain["table"] == self.table)
            })
        });
        if !owned {
            return Err(AlienError::new(unavailable(
                "Refusing to replace a firewall table not owned by this Local runtime",
            )));
        }
        Ok(())
    }
}

fn table<'a>(value: &'a serde_json::Value, name: &str) -> Option<&'a serde_json::Value> {
    value["nftables"]
        .as_array()?
        .iter()
        .filter_map(|entry| entry.get("table"))
        .find(|table| table["family"] == "inet" && table["name"] == name)
}

async fn tables() -> Result<serde_json::Value> {
    let output = nft(&["-j", "list", "tables"], None).await?;
    serde_json::from_slice(&output)
        .into_alien_error()
        .context(unavailable("Cannot read the host firewall table inventory"))
}

async fn nft(arguments: &[&str], input: Option<&str>) -> Result<Vec<u8>> {
    let mut child = Command::new("nft")
        .args(arguments)
        .kill_on_drop(true)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .into_alien_error()
        .context(unavailable(
            "Internet egress requires nftables and host CAP_NET_ADMIN",
        ))?;
    if let Some(input) = input {
        child
            .stdin
            .take()
            .expect("piped stdin")
            .write_all(input.as_bytes())
            .await
            .into_alien_error()
            .context(unavailable("Cannot submit the host egress policy"))?;
    }
    let output = child
        .wait_with_output()
        .await
        .into_alien_error()
        .context(unavailable("Cannot wait for host firewall enforcement"))?;
    if !output.status.success() {
        return Err(AlienError::new(unavailable(format!("Cannot enforce Local internet egress (nftables and host CAP_NET_ADMIN are required): {}", String::from_utf8_lossy(&output.stderr).trim()))));
    }
    Ok(output.stdout)
}
