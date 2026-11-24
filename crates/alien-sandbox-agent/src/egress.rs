//! Fail-closed startup policy. No image code or exec request runs before this completes.

use alien_error::{AlienError, Context, IntoAlienError};

use nlink::netlink::{
    nftables::{
        Chain, ChainType, CmpOp, CtState, Expr, Family, Hook, MetaKey, Policy, Priority, Register,
        Rule,
    },
    Connection, Nftables,
};

use crate::error::{ErrorData, Result};

/// The released object-form `deny` policy keeps its direct, UID-filtered supervisor path. The
/// `allow` and `allowDomains` forms were removed; a startup policy naming one fails to parse and
/// the agent refuses to start.
#[derive(serde::Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum LegacyEgress {
    Deny,
}

/// New string policies are handled by the gateway; object policies retain released behavior.
pub fn legacy_from_env() -> Result<Option<LegacyEgress>> {
    let Ok(encoded) = std::env::var("ALIEN_SANDBOX_EGRESS") else {
        return Ok(None);
    };
    if matches!(encoded.as_str(), "none" | "internet")
        || serde_json::from_str::<String>(&encoded)
            .is_ok_and(|value| matches!(value.as_str(), "none" | "internet"))
    {
        return Ok(None);
    }
    serde_json::from_str(&encoded)
        .map(Some)
        .into_alien_error()
        .context(failed("parse legacy startup policy"))
}

fn failed(reason: impl ToString) -> ErrorData {
    ErrorData::OperationFailed {
        operation: "install sandbox egress policy".to_string(),
        reason: reason.to_string(),
    }
}

/// Preserves the released supervisor policy: only the command UID is filtered. Agent and
/// platform transport traffic keep their existing route; legacy policies never use the gateway.
pub fn install(_policy: &LegacyEgress, command_uid: u32) -> Result<()> {
    if unsafe { libc::geteuid() } != 0 || command_uid == 0 {
        return Err(AlienError::new(failed(
            "requires a root supervisor and a non-root command uid",
        )));
    }
    crate::privilege::block_command_ipv6();
    crate::netfilter::install(async {
        let conn = Connection::<Nftables>::new()?;
        let mut batch = conn.transaction();
        if conn
            .list_tables()
            .await?
            .iter()
            .any(|t| t.name == "alien_egress" && t.family == Family::Ip)
        {
            batch = batch.del_table("alien_egress", Family::Ip);
        }
        let command = || Rule::new("alien_egress", "commands").family(Family::Ip);
        batch = batch
            .add_table("alien_egress", Family::Ip)
            .add_chain(
                Chain::new("alien_egress", "output")?
                    .family(Family::Ip)
                    .chain_type(ChainType::Filter)
                    .hook(Hook::Output)
                    .priority(Priority::Filter)
                    .policy(Policy::Accept),
            )
            .add_chain(Chain::new("alien_egress", "commands")?.family(Family::Ip))
            .add_rule(
                Rule::new("alien_egress", "output")
                    .family(Family::Ip)
                    .expr(Expr::Meta {
                        dreg: Register::R0,
                        key: MetaKey::SkUid,
                    })
                    .expr(Expr::Cmp {
                        sreg: Register::R0,
                        op: CmpOp::Eq,
                        data: command_uid.to_ne_bytes().to_vec(),
                    })
                    .try_jump("commands")?,
            );
        batch = batch
            .add_rule(command().match_udp_dport(53).drop())
            .add_rule(command().match_tcp_dport(53).drop())
            .add_rule(command().match_oif("lo").accept())
            .add_rule(
                command()
                    .match_ct_state(CtState::ESTABLISHED | CtState::RELATED)
                    .accept(),
            );
        batch.add_rule(command().drop()).commit(&conn).await
    })
    .into_alien_error()
    .context(failed("install legacy command egress rules"))
}
