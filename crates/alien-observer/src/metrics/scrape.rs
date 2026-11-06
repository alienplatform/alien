use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr},
    time::Duration,
};

use alien_client_core::ErrorData;
use alien_error::{Context, IntoAlienError};
use nom_openmetrics::{MetricDescriptor, MetricType};
use opentelemetry_proto::tonic::metrics::v1::{
    metric::Data, summary_data_point::ValueAtQuantile, AggregationTemporality, Histogram,
    HistogramDataPoint, Metric, Summary, SummaryDataPoint,
};

use super::{attributes, invalid, point, scalar, MetricBatch, ScalarKind};
use crate::Result;

/// One explicitly configured workload replica. No annotations, URL redirects or network discovery.
pub use alien_core::ScrapeEndpoint;

/// The runtime must resolve an endpoint from an approved workload before invoking this collector.
/// Redirects, URL credentials and cloud metadata endpoints are rejected. Authentication headers
/// belong to the transport owner and are deliberately not accepted from scraped data.
pub async fn collect(endpoint: &ScrapeEndpoint, time_unix_nano: u64) -> Result<MetricBatch> {
    let url =
        reqwest::Url::parse(&endpoint.url).map_err(|_| invalid("Invalid scrape endpoint URL"))?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || endpoint.replica_id.is_empty()
        || endpoint.addresses.is_empty()
        || endpoint.addresses.len() > 16
        || endpoint.addresses.iter().any(forbidden_address)
        || endpoint.max_response_bytes == 0
        || endpoint.max_response_bytes > 16 * 1024 * 1024
        || endpoint.max_samples == 0
        || endpoint.max_samples > 100_000
        || endpoint.timeout_seconds == 0
        || endpoint.timeout_seconds > 60
    {
        return Err(invalid("Invalid scrape endpoint configuration"));
    }
    if matches!(
        url.host_str(),
        Some("169.254.169.254" | "metadata.google.internal" | "168.63.129.16" | "100.100.100.200")
    ) {
        return Err(invalid("Cloud metadata cannot be scraped"));
    }
    let host = url
        .host_str()
        .ok_or_else(|| invalid("Scrape endpoint missing hostname"))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| invalid("Scrape endpoint missing port"))?;
    let addresses: Vec<_> = endpoint
        .addresses
        .iter()
        .map(|address| SocketAddr::new(*address, port))
        .collect();
    if let Ok(literal) = host.parse::<IpAddr>() {
        if !endpoint.addresses.contains(&literal) {
            return Err(invalid(
                "Scrape URL address is not an approved replica address",
            ));
        }
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .resolve_to_addrs(host, &addresses)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(endpoint.timeout_seconds))
        .build()
        .into_alien_error()
        .context(request_error("Could not initialize scrape transport"))?;
    let mut response = client
        .get(url)
        .header(
            "accept",
            "application/openmetrics-text;version=1.0.0, text/plain;version=0.0.4;q=0.9",
        )
        .send()
        .await
        .into_alien_error()
        .context(request_error("Scrape request failed"))?
        .error_for_status()
        .into_alien_error()
        .context(request_error("Scrape endpoint returned an error"))?;
    if !response.status().is_success() {
        return Err(invalid("Scrape endpoint redirected"));
    }
    let openmetrics = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/openmetrics-text"));
    if response
        .content_length()
        .is_some_and(|size| size > endpoint.max_response_bytes as u64)
    {
        return Err(invalid("Scrape exceeds response size limit"));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .into_alien_error()
        .context(request_error("Scrape body read failed"))?
    {
        if bytes.len().saturating_add(chunk.len()) > endpoint.max_response_bytes {
            return Err(invalid("Scrape exceeds response size limit"));
        }
        bytes.extend_from_slice(&chunk);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| invalid("Scrape body is not UTF-8"))?;
    parse(
        text,
        openmetrics,
        &endpoint.replica_id,
        time_unix_nano,
        endpoint.max_samples,
    )
}

fn request_error(message: &str) -> ErrorData {
    ErrorData::InvalidClientConfig {
        message: message.into(),
        errors: None,
    }
}

/// Parses a complete exposition atomically; malformed/unsupported input never becomes a partial success.
pub fn parse(
    text: &str,
    openmetrics: bool,
    replica_id: &str,
    now: u64,
    max_samples: usize,
) -> Result<MetricBatch> {
    let parsed = if openmetrics {
        nom_openmetrics::parser::openmetrics(text)
    } else {
        nom_openmetrics::parser::prometheus(text)
    };
    let (remaining, families) =
        parsed.map_err(|_| invalid("Malformed or unsupported metrics exposition"))?;
    if !remaining.is_empty() {
        return Err(invalid("Metrics exposition has unparsed input"));
    }
    if families
        .iter()
        .map(|family| family.samples.len())
        .sum::<usize>()
        > max_samples
    {
        return Err(invalid("Scrape exceeds sample limit"));
    }
    let mut batch = MetricBatch::default();
    for family in families {
        let descriptor = family
            .descriptors
            .iter()
            .find_map(|descriptor| match descriptor {
                MetricDescriptor::Type { metric, r#type } => Some((*metric, r#type)),
                _ => None,
            });
        let unit = family
            .descriptors
            .iter()
            .find_map(|descriptor| match descriptor {
                MetricDescriptor::Unit { unit, .. } => Some(unit.to_string()),
                _ => None,
            })
            .unwrap_or_default();
        // nom-openmetrics 0.3 retains the standard histogram keyword as Unknown;
        // recognize that descriptor before validating and reconstructing its buckets.
        if let Some((name, MetricType::Histogram | MetricType::Unknown("histogram"))) = descriptor {
            let mut groups: BTreeMap<BTreeMap<String, String>, HistogramParts> = BTreeMap::new();
            for sample in family.samples {
                let mut labels = labels(sample.labels(), replica_id)?;
                let bound = labels.remove("le");
                let parts = groups.entry(labels).or_default();
                if sample.name() == format!("{name}_bucket") {
                    let bound = bound
                        .ok_or_else(|| invalid("Histogram bucket missing le"))?
                        .parse::<f64>()
                        .map_err(|_| invalid("Invalid histogram bound"))?;
                    parts.buckets.push((bound, integer(sample.number())?));
                } else if sample.name() == format!("{name}_sum") {
                    parts.sum = Some(sample.number());
                } else if sample.name() == format!("{name}_count") {
                    parts.count = Some(integer(sample.number())?);
                } else {
                    return Err(invalid("Unsupported histogram sample"));
                }
            }
            let mut points = Vec::new();
            for (labels, mut parts) in groups {
                parts.buckets.sort_by(|a, b| a.0.total_cmp(&b.0));
                let count = parts
                    .count
                    .ok_or_else(|| invalid("Histogram missing count"))?;
                if parts.buckets.last().copied() != Some((f64::INFINITY, count))
                    || parts
                        .buckets
                        .windows(2)
                        .any(|w| w[0].0 >= w[1].0 || w[0].1 > w[1].1)
                {
                    return Err(invalid("Histogram buckets/count are inconsistent"));
                }
                let mut previous = 0;
                let counts = parts
                    .buckets
                    .iter()
                    .map(|(_, count)| {
                        let delta = *count - previous;
                        previous = *count;
                        delta
                    })
                    .collect();
                let bounds = parts
                    .buckets
                    .iter()
                    .take(parts.buckets.len() - 1)
                    .map(|(bound, _)| *bound)
                    .collect();
                let sum = parts.sum.ok_or_else(|| invalid("Histogram missing sum"))?;
                if !sum.is_finite() {
                    return Err(invalid("Histogram sum is non-finite"));
                }
                points.push(HistogramDataPoint {
                    attributes: attributes(&labels),
                    time_unix_nano: now,
                    count,
                    sum: Some(sum),
                    bucket_counts: counts,
                    explicit_bounds: bounds,
                    ..Default::default()
                });
            }
            batch.metrics.push(Metric {
                name: name.into(),
                unit,
                data: Some(Data::Histogram(Histogram {
                    data_points: points,
                    aggregation_temporality: AggregationTemporality::Cumulative as i32,
                })),
                ..Default::default()
            });
        } else if let Some((name, MetricType::Summary | MetricType::Unknown("summary"))) =
            descriptor
        {
            let mut groups: BTreeMap<BTreeMap<String, String>, SummaryParts> = BTreeMap::new();
            for sample in family.samples {
                let mut labels = labels(sample.labels(), replica_id)?;
                let quantile = labels.remove("quantile");
                let parts = groups.entry(labels).or_default();
                if sample.name() == name {
                    let quantile = quantile
                        .ok_or_else(|| invalid("Summary sample missing quantile"))?
                        .parse::<f64>()
                        .map_err(|_| invalid("Invalid summary quantile"))?;
                    if !quantile.is_finite()
                        || !(0.0..=1.0).contains(&quantile)
                        || sample.number() < 0.0
                    {
                        return Err(invalid("Summary quantile is outside OTLP limits"));
                    }
                    parts.quantiles.push((quantile, sample.number()));
                } else if sample.name() == format!("{name}_sum") && quantile.is_none() {
                    if parts.sum.replace(sample.number()).is_some() {
                        return Err(invalid("Duplicate summary sum"));
                    }
                } else if sample.name() == format!("{name}_count") && quantile.is_none() {
                    if parts.count.replace(integer(sample.number())?).is_some() {
                        return Err(invalid("Duplicate summary count"));
                    }
                } else {
                    return Err(invalid("Unsupported summary sample"));
                }
            }
            let mut points = Vec::new();
            for (labels, mut parts) in groups {
                let count = parts
                    .count
                    .ok_or_else(|| invalid("Summary missing count"))?;
                let sum = parts.sum.ok_or_else(|| invalid("Summary missing sum"))?;
                if !sum.is_finite() || sum < 0.0 || (count == 0 && sum != 0.0) {
                    return Err(invalid("Summary sum/count are outside OTLP limits"));
                }
                parts.quantiles.sort_by(|a, b| a.0.total_cmp(&b.0));
                if parts
                    .quantiles
                    .windows(2)
                    .any(|pair| pair[0].0 == pair[1].0)
                {
                    return Err(invalid("Duplicate summary quantile"));
                }
                points.push(SummaryDataPoint {
                    attributes: attributes(&labels),
                    time_unix_nano: now,
                    count,
                    sum,
                    quantile_values: parts
                        .quantiles
                        .into_iter()
                        .map(|(quantile, value)| ValueAtQuantile { quantile, value })
                        .collect(),
                    ..Default::default()
                });
            }
            batch.metrics.push(Metric {
                name: name.into(),
                unit,
                data: Some(Data::Summary(Summary {
                    data_points: points,
                })),
                ..Default::default()
            });
        } else {
            let kind = match descriptor.map(|(_, kind)| kind) {
                Some(MetricType::Counter) => ScalarKind::CumulativeSum,
                Some(MetricType::Histogram | MetricType::Gaugehistogram | MetricType::Summary) => {
                    return Err(invalid("Unsupported gauge histogram exposition"))
                }
                _ => ScalarKind::Gauge,
            };
            for sample in family.samples {
                let labels = labels(sample.labels(), replica_id)?;
                batch.metrics.push(scalar(
                    sample.name().into(),
                    unit.clone(),
                    kind,
                    vec![point(0, now, sample.number(), &labels)?],
                ));
            }
        }
    }
    Ok(batch)
}

fn labels(
    values: &[nom_openmetrics::Label<'_>],
    replica_id: &str,
) -> Result<BTreeMap<String, String>> {
    let mut labels = BTreeMap::new();
    if values.len() > 64 {
        return Err(invalid("Scrape exceeds label limit"));
    }
    for value in values {
        if value.name.len() > 256
            || value.value.len() > 4096
            || labels
                .insert(value.name.into(), value.value.clone())
                .is_some()
        {
            return Err(invalid("Invalid or duplicate scrape label"));
        }
    }
    // Replica identity always comes from the trusted endpoint, never the application label.
    labels.insert("alien.replica.id".into(), replica_id.into());
    Ok(labels)
}

fn integer(value: f64) -> Result<u64> {
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > 9_007_199_254_740_991.0
    {
        return Err(invalid("Invalid histogram integer count"));
    }
    Ok(value as u64)
}

#[derive(Default)]
struct HistogramParts {
    buckets: Vec<(f64, u64)>,
    count: Option<u64>,
    sum: Option<f64>,
}

#[derive(Default)]
struct SummaryParts {
    quantiles: Vec<(f64, f64)>,
    count: Option<u64>,
    sum: Option<f64>,
}

fn forbidden_address(address: &IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            address.is_link_local()
                || address.is_multicast()
                || address.is_unspecified()
                || matches!(address.octets(), [168, 63, 129, 16] | [100, 100, 100, 200])
        }
        IpAddr::V6(address) => {
            address
                .to_ipv4_mapped()
                .is_some_and(|address| forbidden_address(&IpAddr::V4(address)))
                || address.is_unicast_link_local()
                || address.is_multicast()
                || address.is_unspecified()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry_proto::tonic::metrics::v1::metric::Data;

    #[test]
    fn parses_counter_gauge_and_histogram_without_losing_replica_identity() {
        let text = "# TYPE requests_total counter\nrequests_total{status=\"200\",alien.replica.id=\"spoof\"} 9\n# TYPE memory_bytes gauge\nmemory_bytes 42\n# TYPE duration_seconds histogram\nduration_seconds_bucket{le=\"0.1\"} 2\nduration_seconds_bucket{le=\"1\"} 5\nduration_seconds_bucket{le=\"+Inf\"} 6\nduration_seconds_sum 2.5\nduration_seconds_count 6\n";
        // Prometheus label names do not contain dots; malformed application labels fail closed.
        assert!(parse(text, false, "replica-1", 100, 100).is_err());
        let text = text.replace(",alien.replica.id=\"spoof\"", "");
        let batch = parse(&text, false, "replica-1", 100, 100).unwrap();
        assert_eq!(batch.metrics.len(), 3);
        let Some(Data::Sum(counter)) = &batch.metrics[0].data else {
            panic!("counter type lost")
        };
        assert!(counter.is_monotonic);
        assert_eq!(
            counter.aggregation_temporality,
            AggregationTemporality::Cumulative as i32
        );
        assert_eq!(counter.data_points[0].time_unix_nano, 100);
        assert!(counter.data_points[0]
            .attributes
            .iter()
            .any(|label| label.key == "alien.replica.id"));
        let Some(Data::Histogram(histogram)) = &batch.metrics[2].data else {
            panic!("histogram type lost")
        };
        assert_eq!(histogram.data_points[0].bucket_counts, [2, 3, 1]);
        assert_eq!(histogram.data_points[0].explicit_bounds, [0.1, 1.0]);
        assert_eq!(histogram.data_points[0].sum, Some(2.5));
        assert_eq!(histogram.data_points[0].count, 6);
    }

    #[test]
    fn requires_complete_openmetrics_and_enforces_limits() {
        assert!(parse("# TYPE x gauge\nx 1\n# EOF\n", true, "replica", 100, 1).is_ok());
        assert!(parse("# TYPE x gauge\nx 1\n", true, "replica", 100, 1).is_err());
        assert!(parse("x 1\ny 2\n", false, "replica", 100, 1).is_err());
        assert!(parse("x 1\nnot a metric\n", false, "replica", 100, 10).is_err());
        assert!(parse("x{a=\"one\",a=\"two\"} 1\n", false, "replica", 100, 10).is_err());
    }

    #[test]
    fn rejects_inconsistent_histogram_instead_of_emitting_wrong_counts() {
        let text = "# TYPE h histogram\nh_bucket{le=\"1\"} 3\nh_bucket{le=\"+Inf\"} 2\nh_sum 2\nh_count 2\n";
        assert!(parse(text, false, "replica", 100, 10).is_err());
    }

    #[test]
    fn preserves_go_client_summary_quantiles_count_and_sum_without_inventing_buckets() {
        let text = "# HELP go_gc_duration_seconds A summary of wall-time pause in garbage collection cycles.\n# TYPE go_gc_duration_seconds summary\ngo_gc_duration_seconds{quantile=\"0\"} 0.0001\ngo_gc_duration_seconds{quantile=\"0.5\"} 0.0002\ngo_gc_duration_seconds{quantile=\"1\"} 0.0003\ngo_gc_duration_seconds_sum 0.0006\ngo_gc_duration_seconds_count 3\n";
        let batch = parse(text, false, "replica", 123, 100).unwrap();
        assert_eq!(batch.metrics.len(), 1);
        let Some(Data::Summary(summary)) = &batch.metrics[0].data else {
            panic!("summary semantics lost");
        };
        let point = &summary.data_points[0];
        assert_eq!(point.count, 3);
        assert_eq!(point.sum, 0.0006);
        assert_eq!(
            point
                .quantile_values
                .iter()
                .map(|value| value.quantile)
                .collect::<Vec<_>>(),
            vec![0.0, 0.5, 1.0]
        );
        assert_eq!(point.time_unix_nano, 123);
        assert!(!point.attributes.iter().any(|label| label.key == "quantile"));
        assert!(parse(
            "# TYPE s summary\ns{quantile=\"2\"} 1\ns_sum 1\ns_count 1\n",
            false,
            "replica",
            123,
            100
        )
        .is_err());
        assert!(parse(
            "# TYPE s summary\ns_sum 1\ns_sum 2\ns_count 1\n",
            false,
            "replica",
            123,
            100
        )
        .is_err());
    }

    #[test]
    fn blocks_metadata_addresses_including_mapped_ipv6() {
        for value in [
            "169.254.169.254",
            "::ffff:169.254.169.254",
            "168.63.129.16",
            "100.100.100.200",
            "fe80::1",
        ] {
            assert!(forbidden_address(&value.parse().unwrap()), "{value}");
        }
        assert!(!forbidden_address(&"127.0.0.1".parse().unwrap()));
        assert!(!forbidden_address(&"10.42.0.12".parse().unwrap()));
    }
}
