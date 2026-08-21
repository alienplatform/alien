//! Telling the code the agent supervises apart from the caller it serves.
//!
//! In transport mode the agent accepts requests without a capability, because the cloud in front
//! of it already scopes the caller to one sandbox. That holds for anything arriving through the
//! transport. It does not hold for the command the agent itself spawned: that command shares the
//! guest's network stack, so it can reach the same port directly.
//!
//! The discriminator is the connecting socket's owner, not its address. A proxy that terminates
//! inside the guest also connects over loopback, so refusing loopback would refuse the one caller
//! the agent exists to serve. Refusing the exec uid refuses exactly the code being supervised.

use std::net::SocketAddr;

/// Whether transport mode may serve `peer`.
///
/// A caller arriving through the transport connects from off the machine, so its socket is on the
/// far side and this host knows nothing about it. Anything connecting from an address this host
/// holds is inside the guest, and the only process that should be there is the one the agent is
/// running — so an in-guest caller has to prove it is something else.
pub fn transport_may_serve(peer: SocketAddr, exec_uid: u32) -> bool {
    // Attribution reads the kernel's socket table, which only Linux offers. The agent ships in
    // Linux images, and `attribution_works` refuses to start transport mode without it, so this
    // is the development build rather than a mode a sandbox ever runs in.
    if !cfg!(target_os = "linux") {
        return true;
    }

    decide(originates_here(peer), owning_uid(peer), exec_uid)
}

/// The rule itself, without the I/O, so every combination is testable.
///
/// An unattributable in-guest socket is refused rather than served: a command can write its
/// request and close before the socket table is read, and treating that as "not one of ours"
/// would make the check optional for anyone who asks quickly enough.
fn decide(originates_here: bool, owner: Option<u32>, exec_uid: u32) -> bool {
    if !originates_here {
        return true;
    }

    matches!(owner, Some(uid) if uid != exec_uid)
}

/// Whether `peer` is an address this host holds.
///
/// Not a loopback test: a command can reach the agent through the guest's routable address just
/// as easily, and that is still the same machine.
///
/// Asked by binding rather than by walking the interface list, because only an address this host
/// holds can be bound.
fn originates_here(peer: SocketAddr) -> bool {
    let mut probe = peer;
    probe.set_port(0);

    bind_says_local(std::net::UdpSocket::bind(probe).map(|_| ()))
}

/// What a probe's outcome means, without the I/O, so both failure meanings are testable.
///
/// `EADDRNOTAVAIL` is the one answer that says the address is not on this host. Every other
/// failure says the question could not be asked, and that is treated as local so a restricted
/// environment refuses rather than serves.
fn bind_says_local(probe: std::io::Result<()>) -> bool {
    match probe {
        Ok(()) => true,
        Err(error) => error.kind() != std::io::ErrorKind::AddrNotAvailable,
    }
}

/// Whether socket attribution can be performed in this environment.
///
/// Checked once at startup rather than per request, because the per-request answer is ambiguous
/// on its own: an unattributable socket is either a caller arriving through the transport or a
/// table this process cannot read, and only one of those is safe to serve. Establishing the
/// mechanism works up front leaves the first meaning as the only one.
pub fn attribution_works() -> bool {
    !cfg!(target_os = "linux") || std::fs::read_to_string("/proc/net/tcp").is_ok()
}

/// The uid owning the socket whose *local* end is `peer`.
///
/// A connecting socket's local end is the address the accepting side sees as its peer, so the
/// entry is found by matching `peer` against the local column rather than the remote one.
///
/// `None` when `/proc/net/tcp` cannot be read or holds no matching entry, which the caller has to
/// decide about: this is a second lock over path confinement and the uid drop, not the first.
#[cfg(target_os = "linux")]
pub fn owning_uid(peer: SocketAddr) -> Option<u32> {
    for table in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let Ok(contents) = std::fs::read_to_string(table) else {
            continue;
        };

        if let Some(uid) = find_owner(&contents, peer) {
            return Some(uid);
        }
    }

    None
}

#[cfg(not(target_os = "linux"))]
pub fn owning_uid(_peer: SocketAddr) -> Option<u32> {
    None
}

/// The `st` column of a socket that is connected, as `/proc/net/tcp` writes it.
#[cfg(any(target_os = "linux", test))]
const TCP_ESTABLISHED: &str = "01";

/// Scans one `/proc/net/tcp`-format table. Split out so the parsing is testable off Linux.
#[cfg(any(target_os = "linux", test))]
///
/// Columns are `sl local_address rem_address st tx:rx tr:when retrnsmt uid ...`, with addresses
/// as big-endian hex and the port after a colon.
fn find_owner(table: &str, peer: SocketAddr) -> Option<u32> {
    for line in table.lines().skip(1) {
        let columns: Vec<&str> = line.split_whitespace().collect();
        if columns.len() < 8 {
            continue;
        }

        let (address, port) = columns[1].split_once(':')?;
        if u16::from_str_radix(port, 16).ok()? != peer.port() {
            continue;
        }

        // The port alone is not enough: two sockets can share a port across interfaces, and
        // attributing the wrong one would refuse a legitimate caller.
        if !address_matches(address, peer) {
            continue;
        }

        // Only a live socket has an owner. A closed one lingers in TIME_WAIT under uid 0 on the
        // same address and port, and the kernel hands that port to a new connection while the
        // stale row is still listed — reading it would attribute the caller to root and serve it.
        if columns[3] != TCP_ESTABLISHED {
            continue;
        }

        return columns[7].parse().ok();
    }

    None
}

/// Whether a `/proc/net/tcp` address column names the same host as `peer`.
#[cfg(any(target_os = "linux", test))]
///
/// IPv4 is four bytes of little-endian hex; IPv6 is sixteen written as four such words.
fn address_matches(column: &str, peer: SocketAddr) -> bool {
    match peer {
        SocketAddr::V4(peer) => u32::from_str_radix(column, 16)
            .map(|raw| std::net::Ipv4Addr::from(raw.to_be()))
            .is_ok_and(|address| address == *peer.ip()),
        SocketAddr::V6(peer) => {
            if column.len() != 32 {
                return false;
            }
            let mut octets = [0u8; 16];
            for (word, chunk) in column.as_bytes().chunks(8).enumerate() {
                let Ok(raw) = u32::from_str_radix(std::str::from_utf8(chunk).unwrap_or(""), 16)
                else {
                    return false;
                };
                octets[word * 4..word * 4 + 4].copy_from_slice(&raw.to_le_bytes());
            }
            std::net::Ipv6Addr::from(octets) == *peer.ip()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:9224 0100007F:2313 01 00000000:00000000 00:00000000 00000000 60000        0 12345 1
   1: 0100007F:9228 0100007F:2313 01 00000000:00000000 00:00000000 00000000     0        0 12346 1";

    /// Getting this backwards turns every in-guest caller into a remote one, which in transport
    /// mode is the entire check. Loopback pins the local side because it is on every host; the
    /// documentation range pins the other because it is on none.
    #[test]
    fn this_host_recognises_its_own_addresses() {
        assert!(
            originates_here("127.0.0.1:9224".parse().expect("literal")),
            "loopback must be recognised as local, or transport mode serves the code it runs"
        );
        assert!(
            originates_here("[::1]:9224".parse().expect("literal")),
            "the guest reaches the agent over IPv6 loopback just as easily"
        );
        assert!(
            !originates_here("203.0.113.7:9224".parse().expect("literal")),
            "a documentation-range address is not on this host"
        );
    }

    /// A probe that fails is not evidence of anything on its own. Reading "could not ask" as
    /// "remote" would serve the supervised code on any host that refuses the probe.
    #[test]
    fn only_an_unavailable_address_reads_as_remote() {
        use std::io::{Error, ErrorKind};

        assert!(bind_says_local(Ok(())));
        assert!(!bind_says_local(Err(Error::from(ErrorKind::AddrNotAvailable))));
        assert!(bind_says_local(Err(Error::from(ErrorKind::PermissionDenied))));
        assert!(bind_says_local(Err(Error::from(ErrorKind::Unsupported))));
    }

    fn v4(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    /// Every combination of the rule, including the one a command can arrange for itself by
    /// closing its socket before the table is read.
    #[test]
    fn only_an_identified_in_guest_caller_that_is_not_the_supervised_code_is_served() {
        // Off the machine: the transport already scoped it, and its socket is not ours to read.
        assert!(decide(false, None, 60000));
        assert!(decide(false, Some(60000), 60000));

        // In the guest: it has to be something other than the code the agent runs.
        assert!(decide(true, Some(0), 60000), "another user in the guest is a caller");
        assert!(!decide(true, Some(60000), 60000), "the supervised code is not a caller");
        assert!(!decide(true, None, 60000), "an in-guest socket we cannot attribute is refused");
    }

    #[test]
    fn the_socket_owner_is_read_from_the_matching_row() {
        assert_eq!(find_owner(TABLE, v4(0x9224)), Some(60000));
        assert_eq!(find_owner(TABLE, v4(0x9228)), Some(0));
    }

    #[test]
    fn a_port_with_no_entry_is_not_attributed() {
        assert_eq!(find_owner(TABLE, v4(0x9999)), None);
    }

    /// A closed socket stays listed in TIME_WAIT under uid 0, and the kernel reuses its port for
    /// a new connection while that row is still there. The stale row sorts first; attributing
    /// from it would call the supervised code root and serve it.
    #[test]
    fn a_stale_time_wait_row_on_the_same_port_is_skipped() {
        const REUSED: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:C1D4 0100007F:2313 06 00000000:00000000 03:00000A2C 00000000     0        0 0 3
   1: 0100007F:C1D4 0100007F:2314 01 00000000:00000000 00:00000000 00000000 60000        0 12347 1";
        assert_eq!(
            find_owner(REUSED, v4(0xC1D4)),
            Some(60000),
            "the live row owns the socket, not the TIME_WAIT one ahead of it"
        );

        const ONLY_STALE: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:C1D4 0100007F:2313 06 00000000:00000000 03:00000A2C 00000000     0        0 0 3";
        assert_eq!(
            find_owner(ONLY_STALE, v4(0xC1D4)),
            None,
            "a socket that has closed has no owner to attribute"
        );
    }

    /// The port alone would match both rows here. Attributing by port only would report the
    /// sandbox uid for a caller arriving on another interface, and refuse it.
    #[test]
    fn a_matching_port_on_another_address_is_not_attributed() {
        assert_eq!(find_owner(TABLE, SocketAddr::from(([10, 0, 0, 5], 0x9224))), None);
    }
}
