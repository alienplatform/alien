use alien_aws_clients::{
    cloudwatch::{Dimension, GetMetricDataRequest, Metric, MetricDataQuery, MetricStat},
    CloudWatchApi,
};

use super::{invalid, point, scalar, CollectionWindow, MetricBatch, ScalarKind};
use crate::Result;

/// One approved CloudWatch series, including all identity dimensions.
pub use alien_core::AwsMetricSource;

/// Collects all pages. Any partial/error result rejects the batch so its checkpoint stays put.
pub async fn collect(
    client: &dyn CloudWatchApi,
    sources: &[AwsMetricSource],
    window: &CollectionWindow,
) -> Result<MetricBatch> {
    window.validate()?;
    if sources.is_empty() || sources.len() > 500 {
        return Err(invalid("Expected 1–500 CloudWatch series"));
    }
    for source in sources {
        if source.period_seconds < 60
            || source.period_seconds > 86400
            || source.period_seconds % 60 != 0
            || !matches!(
                source.statistic.as_str(),
                "Sum" | "Average" | "Minimum" | "Maximum" | "SampleCount"
            )
            || window.start.timestamp() % i64::from(source.period_seconds.max(1)) != 0
            || window.end.timestamp() % i64::from(source.period_seconds.max(1)) != 0
        {
            return Err(invalid(
                "Invalid CloudWatch source period or unsupported statistic",
            ));
        }
    }
    let queries: Vec<_> = sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            MetricDataQuery::builder()
                .id(format!("m{index}"))
                .metric_stat(
                    MetricStat::builder()
                        .metric(
                            Metric::builder()
                                .namespace(source.namespace.clone())
                                .metric_name(source.metric_name.clone())
                                .dimensions(
                                    source
                                        .dimensions
                                        .iter()
                                        .map(|(name, value)| Dimension {
                                            name: name.clone(),
                                            value: value.clone(),
                                        })
                                        .collect(),
                                )
                                .build(),
                        )
                        .period(source.period_seconds)
                        .stat(source.statistic.clone())
                        .build(),
                )
                .return_data(true)
                .build()
        })
        .collect();
    let mut batch = MetricBatch::default();
    let mut token = None;
    let mut seen = std::collections::BTreeSet::new();
    let mut completed = std::collections::BTreeSet::new();
    for _ in 0..100 {
        let response = client
            .get_metric_data(
                GetMetricDataRequest::builder()
                    .start_time(window.start.timestamp())
                    .end_time(window.end.timestamp())
                    .metric_data_queries(queries.clone())
                    .max_datapoints(10000)
                    .scan_by("TimestampAscending".to_string())
                    .maybe_next_token(token)
                    .build(),
            )
            .await?;
        if !response.messages.is_empty() {
            return Err(invalid(
                "CloudWatch returned collection messages; batch is incomplete",
            ));
        }
        let has_more = response
            .next_token
            .as_ref()
            .is_some_and(|token| !token.is_empty());
        for result in response.metric_data_results {
            let index = result
                .id
                .strip_prefix('m')
                .and_then(|index| index.parse::<usize>().ok())
                .filter(|index| *index < sources.len())
                .ok_or_else(|| invalid("CloudWatch returned unknown query ID"))?;
            if !(matches!(result.status_code.as_deref(), Some("Complete"))
                || (has_more && matches!(result.status_code.as_deref(), Some("PartialData"))))
                || !result.messages.is_empty()
                || result.timestamps.len() != result.values.len()
            {
                return Err(invalid(
                    "CloudWatch returned incomplete or malformed series",
                ));
            }
            if result.status_code.as_deref() == Some("Complete") {
                completed.insert(index);
            } else {
                completed.remove(&index);
            }
            let source = &sources[index];
            let mut labels = source.dimensions.clone();
            labels.insert("source.statistic".into(), source.statistic.clone());
            labels.insert(
                "source.period_seconds".into(),
                source.period_seconds.to_string(),
            );
            let mut points = Vec::new();
            for (time, value) in result.timestamps.into_iter().zip(result.values) {
                // CloudWatch timestamps denote the beginning of the aggregation period.
                let start = u64::try_from(time)
                    .ok()
                    .and_then(|time| time.checked_mul(1_000_000_000))
                    .ok_or_else(|| invalid("Invalid CloudWatch timestamp"))?;
                let end = start
                    .checked_add(source.period_seconds as u64 * 1_000_000_000)
                    .ok_or_else(|| invalid("CloudWatch timestamp overflow"))?;
                if time < window.start.timestamp()
                    || time + i64::from(source.period_seconds) > window.end.timestamp()
                {
                    return Err(invalid(
                        "CloudWatch returned an unclosed or out-of-window aggregation period",
                    ));
                }
                if source.monotonic && value < 0.0 {
                    return Err(invalid("CloudWatch counter increment is negative"));
                }
                points.push(point(start, end, value, &labels)?);
            }
            batch.metrics.push(scalar(
                format!("{}/{}", source.namespace, source.metric_name),
                source.unit.clone(),
                if source.statistic == "Sum" || source.statistic == "SampleCount" {
                    if source.monotonic {
                        ScalarKind::DeltaCounter
                    } else {
                        ScalarKind::DeltaSum
                    }
                } else {
                    ScalarKind::Gauge
                },
                points,
            ));
        }
        token = response.next_token.filter(|token| !token.is_empty());
        match token.as_ref() {
            None if completed.len() == sources.len() => return Ok(batch),
            None => {
                return Err(invalid(
                    "CloudWatch omitted or incompletely returned a requested series",
                ))
            }
            Some(token) if !seen.insert(token.clone()) => {
                return Err(invalid("CloudWatch pagination repeated a token"))
            }
            _ => {}
        }
    }
    Err(invalid("CloudWatch collection exceeded page limit"))
}

pub use alien_core::metric_catalog::aws::*;

#[cfg(test)]
mod tests {
    use super::*;
    use alien_aws_clients::cloudwatch::{
        GetMetricDataResponse, ListMetricsRequest, ListMetricsResponse, PutMetricAlarmRequest,
    };
    use async_trait::async_trait;
    use opentelemetry_proto::tonic::metrics::v1::{metric::Data, AggregationTemporality};
    use std::sync::Mutex;

    #[derive(Debug)]
    struct PagedClient(Mutex<Vec<GetMetricDataResponse>>);
    #[async_trait]
    impl CloudWatchApi for PagedClient {
        async fn get_metric_data(
            &self,
            request: GetMetricDataRequest,
        ) -> alien_client_core::Result<GetMetricDataResponse> {
            assert_eq!(request.metric_data_queries.len(), 1);
            let mut responses = self.0.lock().unwrap();
            if responses.len() == 2 {
                assert!(request.next_token.is_none());
            } else {
                assert_eq!(request.next_token.as_deref(), Some("page-2"));
            }
            Ok(responses.remove(0))
        }
        async fn list_metrics(
            &self,
            _: ListMetricsRequest,
        ) -> alien_client_core::Result<ListMetricsResponse> {
            unreachable!()
        }
        async fn put_metric_alarm(
            &self,
            _: PutMetricAlarmRequest,
        ) -> alien_client_core::Result<()> {
            panic!("collector cannot mutate alarms")
        }
        async fn delete_alarms(&self, _: Vec<String>) -> alien_client_core::Result<()> {
            panic!("collector cannot mutate alarms")
        }
    }

    #[tokio::test]
    async fn paginates_partial_data_preserving_interval_sum_semantics() {
        let client = PagedClient(Mutex::new(vec![
            serde_json::from_value(serde_json::json!({"MetricDataResults":[{"Id":"m0","StatusCode":"PartialData","Timestamps":[60],"Values":[7]}],"NextToken":"page-2"})).unwrap(),
            serde_json::from_value(serde_json::json!({"MetricDataResults":[{"Id":"m0","StatusCode":"Complete","Timestamps":[120],"Values":[9]}]})).unwrap(),
        ]));
        let source = lambda("api").remove(0);
        let batch = collect(
            &client,
            &[source],
            &CollectionWindow {
                start: chrono::DateTime::from_timestamp(60, 0).unwrap(),
                end: chrono::DateTime::from_timestamp(180, 0).unwrap(),
            },
        )
        .await
        .unwrap();
        assert_eq!(batch.metrics.len(), 2);
        let Some(Data::Sum(sum)) = &batch.metrics[0].data else {
            panic!("sum type lost")
        };
        assert_eq!(
            sum.aggregation_temporality,
            AggregationTemporality::Delta as i32
        );
        assert_eq!(sum.data_points[0].start_time_unix_nano, 60_000_000_000);
        assert_eq!(sum.data_points[0].time_unix_nano, 120_000_000_000);
        assert_eq!(client.0.lock().unwrap().len(), 0);
    }
}
