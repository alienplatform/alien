//! No egress for guest sockets, without conntrack.
//!
//! Used where the platform must let the agent reach one host (a persistent storage store) that
//! commands must not reach, on a guest kernel with no connection tracking (Azure). Every socket a
//! command opens carries the guest mark, set at creation by the guest cgroup's program. Leaving
//! any interface but loopback, a marked packet that opens a TCP connection (SYN without ACK) or
//! carries UDP is dropped. Replies on connections that reached a command from outside (SYN-ACK
//! and later) still leave, as without the rule. The agent's own sockets are unmarked and pass.

use nlink::netlink::{
    nftables::{
        Chain, ChainType, CmpOp, Expr, Family, Hook, MetaKey, PayloadBase, Policy, Priority,
        Register, Rule,
    },
    Connection, Nftables,
};

use super::guest_network::GUEST_MARK;

const TABLE: &str = "alien_guest_drop";
const CHAIN: &str = "output";
/// TCP flags byte offset in the TCP header, and the SYN and ACK bits.
const TCP_FLAGS_OFFSET: u32 = 13;
const TCP_SYN: u8 = 0x02;
const TCP_ACK: u8 = 0x10;

/// The two drop rules, in order: new TCP connections, then UDP.
pub(super) fn rules() -> Vec<Rule> {
    let guest_leaving = || {
        Rule::new(TABLE, CHAIN)
            .family(Family::Ip)
            .expr(Expr::Meta {
                dreg: Register::R0,
                key: MetaKey::Mark,
            })
            .expr(Expr::Cmp {
                sreg: Register::R0,
                op: CmpOp::Eq,
                data: GUEST_MARK.to_ne_bytes().to_vec(),
            })
            .match_oif_not("lo")
    };
    vec![
        guest_leaving()
            .match_l4proto(libc::IPPROTO_TCP as u8)
            .expr(Expr::Payload {
                dreg: Register::R0,
                base: PayloadBase::Transport,
                offset: TCP_FLAGS_OFFSET,
                len: 1,
            })
            .expr(Expr::Bitwise {
                sreg: Register::R0,
                dreg: Register::R0,
                len: 1,
                mask: vec![TCP_SYN | TCP_ACK],
                xor: vec![0],
            })
            .expr(Expr::Cmp {
                sreg: Register::R0,
                op: CmpOp::Eq,
                data: vec![TCP_SYN],
            })
            .drop(),
        guest_leaving()
            .match_l4proto(libc::IPPROTO_UDP as u8)
            .drop(),
    ]
}

/// Installs the rules in a table of their own, replacing an earlier copy in one transaction.
pub(super) fn install() -> std::io::Result<()> {
    crate::netfilter::install(async {
        let conn = Connection::<Nftables>::new()?;
        let mut batch = conn.transaction();
        if conn
            .list_tables()
            .await?
            .iter()
            .any(|table| table.family == Family::Ip && table.name == TABLE)
        {
            batch = batch.del_table(TABLE, Family::Ip);
        }
        batch = batch.add_table(TABLE, Family::Ip).add_chain(
            Chain::new(TABLE, CHAIN)?
                .family(Family::Ip)
                .chain_type(ChainType::Filter)
                .hook(Hook::Output)
                .priority(Priority::Filter)
                .policy(Policy::Accept),
        );
        for rule in rules() {
            batch = batch.add_rule(rule);
        }
        batch.commit(&conn).await
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both rules match only the guest mark, only off loopback, and drop; the TCP rule matches a
    /// SYN with no ACK, so a reply to an inbound connection is never one it drops. Whether the
    /// kernel enforces them is the Azure acceptance suite's job (a command cannot reach the
    /// store, by name or by address, while the agent can).
    #[test]
    fn the_rules_drop_new_guest_connections_and_guest_udp_only() {
        let rendered: Vec<String> = rules().iter().map(|rule| format!("{rule:?}")).collect();
        assert_eq!(rendered.len(), 2);
        let guest_mark = format!(
            "Meta {{ dreg: R0, key: Mark }}, Cmp {{ sreg: R0, op: Eq, data: {:?} }}",
            GUEST_MARK.to_ne_bytes()
        );
        for rule in &rendered {
            assert!(rule.contains(&guest_mark), "{rule}");
            assert!(
                rule.contains("key: OifName }, Cmp { sreg: R0, op: Neq, data: [108, 111, 0"),
                "{rule}"
            );
            assert!(
                rule.ends_with(
                    "Verdict(Drop)], position: None, comment: None, key: None, error: None }"
                ),
                "{rule}"
            );
        }
        assert!(
            rendered[0].contains(&format!(
                "mask: [{}], xor: [0] }}, Cmp {{ sreg: R0, op: Eq, data: [{TCP_SYN}] }}",
                TCP_SYN | TCP_ACK
            )),
            "{}",
            rendered[0]
        );
        assert!(
            rendered[1].contains(&format!("data: [{}] }}", libc::IPPROTO_UDP)),
            "{}",
            rendered[1]
        );
    }
}
