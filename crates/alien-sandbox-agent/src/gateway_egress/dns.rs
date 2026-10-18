use std::{collections::HashMap, net::Ipv4Addr, sync::Mutex};

/// Synthetic A records preserve a unique name for transparent CONNECT, without external DNS.
#[derive(Default)]
pub(super) struct Names {
    entries: Mutex<HashMap<Ipv4Addr, String>>,
}
impl Names {
    pub(super) fn name(&self, ip: Ipv4Addr) -> Option<String> {
        self.entries.lock().ok()?.get(&ip).cloned()
    }

    pub(super) fn answer(&self, packet: &[u8]) -> Option<Vec<u8>> {
        if packet.len() < 12
            || packet.len() > 512
            || packet[2] & 0xf8 != 0
            || packet[4..12] != [0, 1, 0, 0, 0, 0, 0, 0]
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
        if name.is_empty()
            || name.len() > 253
            || packet.get(offset + 2..offset + 4)? != [0, 1]
            || offset + 4 != packet.len()
        {
            return None;
        }
        let qtype = u16::from_be_bytes([packet[offset], packet[offset + 1]]);
        let mut reply = packet.to_vec();
        reply[2] = 0x81;
        reply[3] = 0x80;
        if qtype != 1 {
            return Some(reply);
        } // AAAA and non-address questions have no answer.
        let mut entries = self.entries.lock().ok()?;
        let ip = if let Some((ip, _)) = entries.iter().find(|(_, n)| *n == &name) {
            *ip
        } else {
            if entries.len() >= 4096 {
                reply[3] = 0x82;
                return Some(reply);
            }
            let ip = Ipv4Addr::from(u32::from(Ipv4Addr::new(198, 18, 0, 1)) + entries.len() as u32);
            entries.insert(ip, name);
            ip
        };
        reply[7] = 1;
        reply.extend_from_slice(&[0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4]);
        reply.extend_from_slice(&ip.octets());
        Some(reply)
    }
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
}
