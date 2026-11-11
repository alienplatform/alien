//! Bedrock throttles the app actually hit, read from the cloud's own metrics.
//!
//! A model can pass the setup check and still be throttled under real traffic (a new account's
//! quota can be a handful of requests a minute). `AWS/Bedrock` `InvocationThrottles` is
//! published per `ModelId`, which is the inference profile the caller used (for example
//! `us.anthropic.claude-sonnet-4-6` or `global.anthropic.claude-sonnet-4-6`). The read is
//! read-only and never changes health by itself.

use std::collections::HashMap;

use alien_aws_clients::cloudwatch::{
    CloudWatchApi, DimensionFilter, GetMetricDataRequest, ListMetricsRequest, ListMetricsResponse,
    Metric, MetricDataQuery, MetricStat,
};
use alien_error::{AlienError, Context};
use chrono::Utc;

use crate::error::{ErrorData, Result};

const NAMESPACE: &str = "AWS/Bedrock";
const METRIC: &str = "InvocationThrottles";
const WINDOW_SECONDS: i64 = 3600;

/// `ModelId` dimension values that belong to `upstream_id`: the bare id and any inference
/// profile (`us.`, `eu.`, `global.`, …) of it.
fn is_profile_of(model_id: &str, upstream_id: &str) -> bool {
    model_id == upstream_id
        || model_id
            .split_once('.')
            .is_some_and(|(prefix, rest)| prefix.len() <= 6 && rest == upstream_id)
}

pub(crate) fn matching_model_ids(metrics: &[Metric], upstream_id: &str) -> Vec<String> {
    let mut ids: Vec<String> = metrics
        .iter()
        .flat_map(|metric| metric.dimensions.iter())
        .filter(|dimension| {
            dimension.name == "ModelId" && is_profile_of(&dimension.value, upstream_id)
        })
        .map(|dimension| dimension.value.clone())
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

/// Throttled requests in the last hour per upstream model id. A model with no throttle metric
/// at all reports `Some(0)`: the metric only exists once the cloud has throttled something,
/// and a successful listing means we can say none were.
pub(crate) async fn throttles_for(
    client: &dyn CloudWatchApi,
    upstream_ids: &[&str],
) -> Result<HashMap<String, u64>> {
    if upstream_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let mut metrics = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let page = client
            .list_metrics(
                ListMetricsRequest::builder()
                    .namespace(NAMESPACE.to_string())
                    .metric_name(METRIC.to_string())
                    .recently_active("PT3H".to_string())
                    .dimensions(vec![DimensionFilter::builder()
                        .name("ModelId".to_string())
                        .build()])
                    .maybe_next_token(token.take())
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Could not list Bedrock throttle metrics".to_string(),
                resource_id: None,
            })?;
        metrics.extend(page.metrics);
        match page.next_token {
            Some(next) => token = Some(next),
            None => break,
        }
    }
    let listed = ListMetricsResponse {
        metrics,
        next_token: None,
        owning_accounts: Vec::new(),
    };
    let mut queries = Vec::new();
    let mut owner: HashMap<String, &str> = HashMap::new();
    for upstream in upstream_ids {
        for (index, model_id) in matching_model_ids(&listed.metrics, upstream)
            .into_iter()
            .enumerate()
        {
            let id = format!("q{}", owner.len());
            owner.insert(id.clone(), upstream);
            let _ = index;
            queries.push(
                MetricDataQuery::builder()
                    .id(id)
                    .metric_stat(
                        MetricStat::builder()
                            .metric(
                                Metric::builder()
                                    .namespace(NAMESPACE.to_string())
                                    .metric_name(METRIC.to_string())
                                    .dimensions(vec![
                                        alien_aws_clients::cloudwatch::Dimension::builder()
                                            .name("ModelId".to_string())
                                            .value(model_id)
                                            .build(),
                                    ])
                                    .build(),
                            )
                            .period(WINDOW_SECONDS as i32)
                            .stat("Sum".to_string())
                            .build(),
                    )
                    .build(),
            );
        }
    }
    let mut totals: HashMap<String, u64> = upstream_ids
        .iter()
        .map(|upstream| ((*upstream).to_string(), 0))
        .collect();
    if queries.is_empty() {
        return Ok(totals);
    }
    let now = Utc::now().timestamp();
    let data = client
        .get_metric_data(
            GetMetricDataRequest::builder()
                .start_time(now - WINDOW_SECONDS)
                .end_time(now)
                .metric_data_queries(queries)
                .build(),
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: "Could not read Bedrock throttle metrics".to_string(),
            resource_id: None,
        })?;
    // A result that is not complete (or a query message) is an unfinished read, not a zero.
    let incomplete = data.next_token.is_some()
        || !data.messages.is_empty()
        || data.metric_data_results.iter().any(|result| {
            result
                .status_code
                .as_deref()
                .is_some_and(|code| code != "Complete")
                || !result.messages.is_empty()
        });
    if incomplete {
        return Err(AlienError::new(ErrorData::CloudPlatformError {
            message: "Bedrock throttle metrics were returned incompletely".to_string(),
            resource_id: None,
        }));
    }
    for result in data.metric_data_results {
        if let Some(upstream) = owner.get(&result.id) {
            let sum: f64 = result.values.iter().sum();
            *totals.entry((*upstream).to_string()).or_default() += sum.max(0.0) as u64;
        }
    }
    Ok(totals)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_aws_clients::cloudwatch::{
        Dimension, GetMetricDataResponse, ListMetricsResponse, MetricDataResult, MockCloudWatchApi,
    };

    fn metric(model_id: &str) -> Metric {
        Metric::builder()
            .namespace(NAMESPACE.to_string())
            .metric_name(METRIC.to_string())
            .dimensions(vec![Dimension::builder()
                .name("ModelId".to_string())
                .value(model_id.to_string())
                .build()])
            .build()
    }

    #[test]
    fn profiles_of_a_model_match_but_other_models_do_not() {
        let metrics = vec![
            metric("global.anthropic.claude-sonnet-4-6"),
            metric("us.anthropic.claude-sonnet-4-6"),
            metric("us.anthropic.claude-haiku-4-5-20251001-v1:0"),
        ];
        assert_eq!(
            matching_model_ids(&metrics, "anthropic.claude-sonnet-4-6"),
            vec![
                "global.anthropic.claude-sonnet-4-6".to_string(),
                "us.anthropic.claude-sonnet-4-6".to_string()
            ]
        );
    }

    #[tokio::test]
    async fn an_incomplete_query_result_is_an_error_not_zero() {
        let mut client = MockCloudWatchApi::new();
        client.expect_list_metrics().returning(|_| {
            Ok(ListMetricsResponse {
                metrics: vec![metric("us.anthropic.claude-sonnet-4-6")],
                next_token: None,
                owning_accounts: vec![],
            })
        });
        client.expect_get_metric_data().returning(|_| {
            Ok(GetMetricDataResponse {
                metric_data_results: vec![MetricDataResult {
                    id: "q0".into(),
                    label: None,
                    status_code: Some("PartialData".into()),
                    timestamps: vec![],
                    values: vec![],
                    messages: vec![],
                }],
                messages: vec![],
                next_token: None,
            })
        });
        assert!(throttles_for(&client, &["anthropic.claude-sonnet-4-6"])
            .await
            .is_err());
    }

    #[tokio::test]
    async fn metrics_on_a_later_page_are_found() {
        let mut client = MockCloudWatchApi::new();
        let mut calls = 0;
        client.expect_list_metrics().returning(move |_| {
            calls += 1;
            Ok(if calls == 1 {
                ListMetricsResponse {
                    metrics: vec![],
                    next_token: Some("page2".into()),
                    owning_accounts: vec![],
                }
            } else {
                ListMetricsResponse {
                    metrics: vec![metric("us.anthropic.claude-sonnet-4-6")],
                    next_token: None,
                    owning_accounts: vec![],
                }
            })
        });
        client.expect_get_metric_data().returning(|_| {
            Ok(GetMetricDataResponse {
                metric_data_results: vec![MetricDataResult {
                    id: "q0".into(),
                    label: None,
                    status_code: Some("Complete".into()),
                    timestamps: vec![1],
                    values: vec![2.0],
                    messages: vec![],
                }],
                messages: vec![],
                next_token: None,
            })
        });
        let totals = throttles_for(&client, &["anthropic.claude-sonnet-4-6"])
            .await
            .unwrap();
        assert_eq!(totals["anthropic.claude-sonnet-4-6"], 2);
    }

    #[tokio::test]
    async fn throttles_are_summed_over_profiles_and_missing_metrics_are_zero() {
        let mut client = MockCloudWatchApi::new();
        client.expect_list_metrics().returning(|_| {
            Ok(ListMetricsResponse {
                metrics: vec![metric("global.anthropic.claude-sonnet-4-6")],
                next_token: None,
                owning_accounts: vec![],
            })
        });
        client.expect_get_metric_data().returning(|_| {
            Ok(GetMetricDataResponse {
                metric_data_results: vec![MetricDataResult {
                    id: "q0".into(),
                    label: None,
                    status_code: None,
                    timestamps: vec![1],
                    values: vec![3.0, 4.0],
                    messages: vec![],
                }],
                messages: vec![],
                next_token: None,
            })
        });
        let totals = throttles_for(
            &client,
            &[
                "anthropic.claude-sonnet-4-6",
                "anthropic.claude-haiku-4-5-20251001-v1:0",
            ],
        )
        .await
        .unwrap();
        assert_eq!(totals["anthropic.claude-sonnet-4-6"], 7);
        assert_eq!(totals["anthropic.claude-haiku-4-5-20251001-v1:0"], 0);
    }
}
