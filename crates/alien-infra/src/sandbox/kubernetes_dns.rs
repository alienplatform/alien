//! The cluster DNS address an allow-egress sandbox's NetworkPolicy admits.
//!
//! The chart admits UDP and TCP port 53 to the addresses in `sandbox.clusterDNS` and to nothing
//! else on private ranges, so a session can resolve names without reaching other cluster hosts.
//! The Operator reads its own `/etc/resolv.conf`, which is what the kubelet gave every pod in the
//! cluster, and refuses to start a new session when the chart's value does not match it: a session
//! that cannot resolve names fails far from the cause.
//!
//! Running sessions are not touched. The check is on create only.

use std::collections::BTreeSet;
use std::net::Ipv4Addr;

/// The IPv4 nameservers in a `resolv.conf`.
pub(crate) fn nameservers(resolv_conf: &str) -> BTreeSet<Ipv4Addr> {
    resolv_conf
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            (words.next() == Some("nameserver")).then(|| words.next())?
        })
        .filter_map(|address| address.parse().ok())
        .collect()
}

/// Whether the configured addresses (`ALIEN_SANDBOX_CLUSTER_DNS`, comma separated) cover every
/// nameserver the Operator itself resolves with. Returns the message to refuse a create with.
pub(crate) fn check(configured: Option<&str>, resolv_conf: &str) -> Result<(), String> {
    let wanted = nameservers(resolv_conf);
    let configured: BTreeSet<Ipv4Addr> = configured
        .unwrap_or_default()
        .split(',')
        .filter_map(|address| address.trim().parse().ok())
        .collect();
    if wanted.is_empty() {
        // Nothing to compare with (an IPv6-only resolver, or no file): the chart's value stands.
        return if configured.is_empty() {
            Err(missing(&wanted))
        } else {
            Ok(())
        };
    }
    if wanted.is_subset(&configured) {
        return Ok(());
    }
    Err(missing(&wanted))
}

fn missing(wanted: &BTreeSet<Ipv4Addr>) -> String {
    let addresses = wanted
        .iter()
        .map(|address| format!("\"{address}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let set_to = if addresses.is_empty() {
        "the cluster DNS service address".to_string()
    } else {
        format!("[{addresses}]")
    };
    format!(
        "this sandbox uses allow egress, and its network policy admits DNS only to the addresses \
         in sandbox.clusterDNS, which do not include this cluster's resolver. Set \
         sandbox.clusterDNS={set_to} (find it with: kubectl run dns-probe --rm -i \
         --restart=Never --image=busybox -- grep nameserver /etc/resolv.conf) and upgrade the chart. Sessions already running are not \
         affected."
    )
}

/// The Operator's own resolver file.
pub(crate) fn read_resolv_conf() -> String {
    std::fs::read_to_string("/etc/resolv.conf").unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUTO_MODE: &str = "search spike.svc.cluster.local svc.cluster.local cluster.local\nnameserver 10.100.0.10\noptions ndots:5\n";

    #[test]
    fn the_configured_resolver_matches() {
        assert!(check(Some("10.100.0.10"), AUTO_MODE).is_ok());
        assert!(check(Some("10.100.0.10, 169.254.20.10"), AUTO_MODE).is_ok());
    }

    #[test]
    fn a_wrong_or_missing_value_names_the_value_and_the_address_to_set() {
        for configured in [None, Some(""), Some("10.96.0.10")] {
            let error = check(configured, AUTO_MODE).unwrap_err();
            assert!(error.contains("sandbox.clusterDNS"), "{error}");
            assert!(error.contains("\"10.100.0.10\""), "{error}");
        }
    }
}
