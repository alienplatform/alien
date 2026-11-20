//! What the agent found and set up at boot for an image that declares permissions, one fact per
//! line, in a root-owned, world-readable file. Written on success as well as on failure, so the
//! state of a running sandbox can be read from inside it.

use std::path::Path;
use std::sync::Mutex;

/// The record.
pub const PATH: &str = "/run/alien/boot-report";

static FACTS: Mutex<Vec<(&'static str, String)>> = Mutex::new(Vec::new());

/// Records (or replaces) one fact, logs it and rewrites [`PATH`]. Called only when the image
/// declares permissions.
pub fn set(key: &'static str, value: String) {
    let value: String = value.chars().filter(|c| *c != '\n').collect();
    // On stderr as well: the agent's default log filter keeps only errors.
    tracing::info!(fact = key, %value, "boot report");
    eprintln!("alien-sandbox-agent: boot report: {key}: {value}");
    let Ok(mut facts) = FACTS.lock() else {
        return;
    };
    match facts.iter_mut().find(|(existing, _)| *existing == key) {
        Some(fact) => fact.1 = value,
        None => facts.push((key, value)),
    }
    let text = render(&facts);
    if let Err(error) = crate::boot_error::write(Path::new(PATH), text.trim_end()) {
        tracing::error!(%error, path = PATH, "cannot write the boot report");
    }
}

fn render(facts: &[(&'static str, String)]) -> String {
    facts
        .iter()
        .map(|(key, value)| format!("{key}: {value}\n"))
        .collect()
}

/// The kernel's IPv6 view: interfaces with an IPv6 address and whether a default route exists.
pub fn record_ipv6() {
    let interfaces = match std::fs::read_to_string("/proc/net/if_inet6") {
        Ok(listing) => ipv6_interfaces(&listing).join(" "),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            "none (no IPv6 in the kernel)".into()
        }
        Err(error) => format!("unreadable ({error})"),
    };
    set("ipv6 addresses (/proc/net/if_inet6)", interfaces);
    let route = match std::fs::read_to_string("/proc/net/ipv6_route") {
        Ok(table) if has_default_route(&table) => "present".into(),
        Ok(_) => "absent".into(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            "absent (no IPv6 in the kernel)".into()
        }
        Err(error) => format!("unknown ({error})"),
    };
    set("ipv6 default route (/proc/net/ipv6_route)", route);
}

/// `name=address` for each line of a `/proc/net/if_inet6` listing.
fn ipv6_interfaces(listing: &str) -> Vec<String> {
    listing
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let (address, name) = (fields.first()?, fields.get(5)?);
            Some(format!("{name}={address}"))
        })
        .collect()
}

/// A route to `::/0` in a `/proc/net/ipv6_route` table (destination all zeros, prefix length 0)
/// that is not the kernel's unreachable placeholder on `lo`.
fn has_default_route(table: &str) -> bool {
    table.lines().any(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        fields.len() >= 10
            && fields[0].bytes().all(|b| b == b'0')
            && fields[1] == "00"
            && fields[9] != "lo"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interfaces_and_default_route_are_read_from_proc() {
        let listing = "00000000000000000000000000000001 01 80 10 80       lo\n\
            fe80000000000000020000fffe000001 02 40 20 80     eth0\n";
        assert_eq!(
            ipv6_interfaces(listing),
            [
                "lo=00000000000000000000000000000001",
                "eth0=fe80000000000000020000fffe000001"
            ]
        );
        let unreachable = "00000000000000000000000000000000 00 00000000000000000000000000000000 00 00000000000000000000000000000000 ffffffff 00000001 00000000 00200200       lo\n";
        assert!(!has_default_route(unreachable));
        let default = "00000000000000000000000000000000 00 00000000000000000000000000000000 00 fe800000000000000000000000000001 00000400 00000001 00000000 00450003     eth0\n";
        assert!(has_default_route(default));
        let link = "fe800000000000000000000000000000 40 00000000000000000000000000000000 00 00000000000000000000000000000000 00000100 00000001 00000000 00000001     eth0\n";
        assert!(!has_default_route(link));
    }
}
