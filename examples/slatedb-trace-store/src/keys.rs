use crate::{
    error::ApiError,
    models::{Trace, TraceQuery},
};

const AGENT_BIT: u8 = 1;
const STATUS_BIT: u8 = 2;
const MODEL_BIT: u8 = 4;

pub fn primary_key(trace_id: &str) -> String {
    format!("t/{}", hex::encode(trace_id))
}

pub fn index_keys(trace: &Trace) -> impl Iterator<Item = String> + '_ {
    (0..8).map(|mask| index_key(mask, trace))
}

fn index_key(mask: u8, trace: &Trace) -> String {
    format!(
        "{}{time:016x}/{}",
        index_prefix(
            mask,
            (mask & AGENT_BIT != 0).then_some(trace.agent.as_str()),
            (mask & STATUS_BIT != 0).then_some(trace.status.as_str()),
            (mask & MODEL_BIT != 0).then_some(trace.model.as_str()),
        ),
        hex::encode(&trace.trace_id),
        time = trace.started_at.timestamp_millis() as u64,
    )
}

pub fn query_prefix(query: &TraceQuery) -> String {
    let mask = (u8::from(query.agent.is_some()) * AGENT_BIT)
        | (u8::from(query.status.is_some()) * STATUS_BIT)
        | (u8::from(query.model.is_some()) * MODEL_BIT);
    index_prefix(
        mask,
        query.agent.as_deref(),
        query.status.as_deref(),
        query.model.as_deref(),
    )
}

fn index_prefix(
    mask: u8,
    agent: Option<&str>,
    status: Option<&str>,
    model: Option<&str>,
) -> String {
    let mut prefix = format!("i/{mask:x}/");
    for value in [agent, status, model].into_iter().flatten() {
        prefix.push_str(&hex::encode(value));
        prefix.push('/');
    }
    prefix
}

pub fn timestamp_from_index_key(key: &[u8], prefix: &str) -> std::result::Result<i64, ApiError> {
    let key = std::str::from_utf8(key).map_err(|_| ApiError::invalid_cursor())?;
    let suffix = key
        .strip_prefix(prefix)
        .ok_or_else(ApiError::invalid_cursor)?;
    let encoded = suffix
        .split('/')
        .next()
        .ok_or_else(ApiError::invalid_cursor)?;
    u64::from_str_radix(encoded, 16)
        .map(|value| value as i64)
        .map_err(|_| ApiError::invalid_cursor())
}

pub fn decode_cursor(cursor: Option<&str>) -> std::result::Result<Option<Vec<u8>>, ApiError> {
    cursor
        .map(|value| hex::decode(value).map_err(|_| ApiError::invalid_cursor()))
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    fn trace() -> Trace {
        Trace {
            trace_id: "trace/one".to_string(),
            agent: "research/agent".to_string(),
            status: "complete".to_string(),
            model: "claude-sonnet".to_string(),
            started_at: chrono::Utc.timestamp_millis_opt(1_700_000_000_000).unwrap(),
            finished_at: None,
            payload: json!({}),
        }
    }

    #[test]
    fn writes_one_index_for_every_filter_subset() {
        let keys = index_keys(&trace()).collect::<Vec<_>>();

        assert_eq!(keys.len(), 8);
        assert_eq!(keys.iter().filter(|key| key.starts_with("i/7/")).count(), 1);
    }

    #[test]
    fn query_prefix_matches_corresponding_index() {
        let query = TraceQuery {
            agent: Some("research/agent".to_string()),
            model: Some("claude-sonnet".to_string()),
            ..TraceQuery::default()
        };
        let prefix = query_prefix(&query);

        assert!(index_keys(&trace()).any(|key| key.starts_with(&prefix)));
    }
}
