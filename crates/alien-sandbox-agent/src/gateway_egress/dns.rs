use std::{
    collections::HashMap,
    io,
    net::{Ipv4Addr, SocketAddr},
    os::fd::AsRawFd,
    sync::Mutex,
    time::Instant,
};
use tokio::{
    net::UdpSocket,
    time::{timeout, Duration},
};

/// Synthetic A records preserve a unique name for transparent CONNECT, without external DNS.
#[derive(Default)]
pub(super) struct Names {
    state: Mutex<NameState>,
}

#[derive(Default)]
struct NameState {
    entries: HashMap<Ipv4Addr, NameEntry>,
    allocated: u32,
}

struct NameEntry {
    name: String,
    expires: Instant,
}

const DNS_TTL: u32 = 60;
const MAX_NAMES: usize = 4096;
const MAX_ADDRESSES: u32 = (1 << 17) - 2;

impl Names {
    pub(super) fn name(&self, ip: Ipv4Addr) -> Option<String> {
        self.state
            .lock()
            .ok()?
            .entries
            .get(&ip)
            .map(|entry| entry.name.clone())
    }

    pub(super) fn answer(&self, packet: &[u8]) -> Option<Vec<u8>> {
        self.answer_at(packet, Instant::now())
    }

    fn answer_at(&self, packet: &[u8], now: Instant) -> Option<Vec<u8>> {
        if packet.len() < 12
            || packet.len() > 4096
            || packet[2] & 0xf8 != 0
            || packet[4..10] != [0, 1, 0, 0, 0, 0]
            || packet[10] != 0
            || packet[11] > 1
        {
            return None;
        }
        let mut offset = 12;
        let mut labels = Vec::new();
        loop {
            let len = usize::from(*packet.get(offset)?);
            offset += 1;
            if len == 0 {
                break;
            }
            if len > 63 {
                return None;
            }
            let label = std::str::from_utf8(packet.get(offset..offset + len)?).ok()?;
            if !label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            {
                return None;
            }
            labels.push(label.to_ascii_lowercase());
            offset += len;
        }
        let name = labels.join(".");
        if name.is_empty() || name.len() > 253 || packet.get(offset + 2..offset + 4)? != [0, 1] {
            return None;
        }
        let question_end = offset + 4;
        if packet[11] == 1 {
            // Accept one EDNS OPT pseudo-record, but do not copy arbitrary options into replies.
            let opt = packet.get(question_end..)?;
            if opt.len() < 11
                || opt[..3] != [0, 0, 41]
                || 11 + usize::from(u16::from_be_bytes([opt[9], opt[10]])) != opt.len()
            {
                return None;
            }
        } else if question_end != packet.len() {
            return None;
        }
        let qtype = u16::from_be_bytes([packet[offset], packet[offset + 1]]);
        let mut reply = packet[..question_end].to_vec();
        reply[11] = 0;
        reply[2] = 0x81;
        reply[3] = 0x80;
        if qtype != 1 {
            return Some(reply);
        } // AAAA and non-address questions have no answer.
        let mut state = self.state.lock().ok()?;
        let expires = now + Duration::from_secs(u64::from(DNS_TTL));
        let ip = if let Some((ip, entry)) = state.entries.iter_mut().find(|(_, e)| e.name == name) {
            entry.expires = expires;
            *ip
        } else {
            if state.entries.len() >= MAX_NAMES {
                state.entries.retain(|_, entry| entry.expires > now);
            }
            if state.entries.len() >= MAX_NAMES || state.allocated == MAX_ADDRESSES {
                reply[3] = 0x82;
                return Some(reply);
            }
            // Never recycle an address: clients can retain an expired answer indefinitely.
            // Exhausting 198.18.0.0/15 fails closed instead of retargeting a stale connection.
            let ip = Ipv4Addr::from(u32::from(Ipv4Addr::new(198, 18, 0, 1)) + state.allocated);
            state.allocated += 1;
            state.entries.insert(ip, NameEntry { name, expires });
            ip
        };
        reply[7] = 1;
        reply.extend_from_slice(&[0xc0, 0x0c, 0, 1, 0, 1]);
        reply.extend_from_slice(&DNS_TTL.to_be_bytes());
        reply.extend_from_slice(&[0, 4]);
        reply.extend_from_slice(&ip.octets());
        Some(reply)
    }
}

/// Resolve the trusted gateway hostname through a marked control socket. Guest DNS never uses
/// this path: it receives synthetic records and cannot send externally observable DNS questions.
pub(super) async fn resolve_gateway(
    authority: &str,
    marked: bool,
) -> std::io::Result<std::net::SocketAddr> {
    if let Ok(address) = authority.parse::<SocketAddr>() {
        if address.is_ipv4() {
            return Ok(address);
        }
    }
    let (host, port) = authority
        .rsplit_once(':')
        .ok_or_else(|| io::Error::other("gateway needs host:port"))?;
    let port = port
        .parse::<u16>()
        .map_err(|_| io::Error::other("invalid gateway port"))?;
    if port == 0 || host.len() > 253 {
        return Err(io::Error::other("invalid gateway authority"));
    }
    let mut query = vec![0u8; 12];
    let random = uuid::Uuid::new_v4();
    query[..2].copy_from_slice(&random.as_bytes()[..2]);
    query[2] = 1;
    query[5] = 1;
    for label in host.trim_end_matches('.').split('.') {
        if label.is_empty()
            || label.len() > 63
            || !label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(io::Error::other("invalid gateway DNS name"));
        }
        query.push(label.len() as u8);
        query.extend_from_slice(label.as_bytes());
    }
    query.extend_from_slice(&[0, 0, 1, 0, 1]);
    let resolver = std::fs::read_to_string("/etc/resolv.conf")?
        .lines()
        .find_map(|line| {
            let mut words = line.split_whitespace();
            (words.next() == Some("nameserver"))
                .then(|| words.next()?.parse::<Ipv4Addr>().ok())
                .flatten()
        })
        .ok_or_else(|| io::Error::other("no IPv4 control DNS resolver"))?;
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    if marked {
        super::redirect::mark(socket.as_raw_fd())?;
    }
    socket.connect((resolver, 53)).await?;
    socket.send(&query).await?;
    let mut answer = [0u8; 4096];
    let n = timeout(Duration::from_secs(3), socket.recv(&mut answer))
        .await
        .map_err(|_| io::Error::other("gateway DNS timeout"))??;
    let answer = &answer[..n];
    if n < query.len()
        || answer[..2] != query[..2]
        || answer[2] & 0x82 != 0x80
        || answer[3] & 15 != 0
        || answer[4..6] != [0, 1]
        || answer[12..query.len()] != query[12..]
    {
        return Err(io::Error::other("invalid gateway DNS reply"));
    }
    let count = u16::from_be_bytes([answer[6], answer[7]]);
    let mut offset = query.len();
    for _ in 0..count {
        loop {
            let len = *answer
                .get(offset)
                .ok_or_else(|| io::Error::other("short DNS name"))? as usize;
            offset += 1;
            if len == 0 {
                break;
            }
            if len & 0xc0 == 0xc0 {
                offset += 1;
                break;
            }
            if len > 63 {
                return Err(io::Error::other("invalid DNS name"));
            }
            offset += len;
        }
        let record = answer
            .get(offset..offset + 10)
            .ok_or_else(|| io::Error::other("short DNS record"))?;
        let len = u16::from_be_bytes([record[8], record[9]]) as usize;
        offset += 10;
        let data = answer
            .get(offset..offset + len)
            .ok_or_else(|| io::Error::other("short DNS data"))?;
        if record[..4] == [0, 1, 0, 1] && len == 4 {
            return Ok(SocketAddr::from((
                [data[0], data[1], data[2], data[3]],
                port,
            )));
        }
        offset += len;
    }
    Err(io::Error::other("gateway has no IPv4 address"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn question(name: &str) -> Vec<u8> {
        let mut q = vec![0, 7, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        for label in name.split('.') {
            q.push(label.len() as u8);
            q.extend_from_slice(label.as_bytes());
        }
        q.extend_from_slice(&[0, 0, 1, 0, 1]);
        q
    }
    #[test]
    fn names_get_distinct_stable_addresses_without_resolution() {
        let names = Names::default();
        let first = names.answer(&question("a.example.com")).unwrap();
        let second = names.answer(&question("b.example.com")).unwrap();
        assert_ne!(&first[first.len() - 4..], &second[second.len() - 4..]);
        assert_eq!(first, names.answer(&question("a.example.com")).unwrap());
        assert_eq!(
            names.name(Ipv4Addr::new(198, 18, 0, 1)).as_deref(),
            Some("a.example.com")
        );
        assert!(names.answer(&[0; 512]).is_none());
    }

    #[test]
    fn edns_queries_preserve_name_mapping_without_forwarding_options() {
        let names = Names::default();
        let plain = question("example.com");
        let mut edns = plain.clone();
        edns[11] = 1;
        edns.extend_from_slice(&[0, 0, 41, 4, 208, 0, 0, 0, 0, 0, 0]);
        assert_eq!(names.answer(&plain), names.answer(&edns));
        edns.pop();
        assert!(names.answer(&edns).is_none());
    }

    #[test]
    fn expired_names_free_capacity_without_retargeting_old_addresses() {
        let names = Names::default();
        let start = Instant::now();
        for i in 0..MAX_NAMES {
            let reply = names
                .answer_at(&question(&format!("n{i}.example.com")), start)
                .unwrap();
            assert_eq!(reply[7], 1);
        }
        let extra = question("extra.example.com");
        assert_eq!(names.answer_at(&extra, start).unwrap()[3] & 15, 2);
        let first_ip = Ipv4Addr::new(198, 18, 0, 1);
        let refreshed = start + Duration::from_secs(30);
        names
            .answer_at(&question("n0.example.com"), refreshed)
            .unwrap();
        let expired = start + Duration::from_secs(60);
        let reply = names.answer_at(&extra, expired).unwrap();
        let new_ip = Ipv4Addr::from(<[u8; 4]>::try_from(&reply[reply.len() - 4..]).unwrap());
        assert_eq!(names.name(new_ip).as_deref(), Some("extra.example.com"));
        assert_eq!(names.name(first_ip).as_deref(), Some("n0.example.com"));
        assert_eq!(names.name(Ipv4Addr::new(198, 18, 0, 2)), None);
        assert_eq!(names.name(first_ip), Some("n0.example.com".to_string()));
        assert!(u32::from(new_ip) >= u32::from(first_ip) + MAX_NAMES as u32);
    }

    #[test]
    fn synthetic_address_exhaustion_never_wraps_into_an_old_mapping() {
        let names = Names::default();
        names.state.lock().unwrap().allocated = MAX_ADDRESSES - 1;
        let reply = names.answer(&question("last.example.com")).unwrap();
        assert_eq!(&reply[reply.len() - 4..], &[198, 19, 255, 254]);
        assert_eq!(
            names.answer(&question("overflow.example.com")).unwrap()[3] & 15,
            2
        );
        assert_eq!(
            names.name(Ipv4Addr::new(198, 19, 255, 254)).as_deref(),
            Some("last.example.com")
        );
    }
}
