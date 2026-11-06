//! Telemetry ingestion endpoints.
//!
//! Accepts OTLP protobuf data and forwards to the TelemetryBackend.
//! Scope and source are extracted exclusively from the authenticated subject.

use axum::{
    body::Bytes,
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Response},
    Json,
};

use crate::auth::{Role, Scope, TelemetryCapability};
use crate::error::ErrorData;
use crate::traits::{TelemetryCaller, TelemetrySignal};

use super::{auth, AppState};

/// POST /v1/logs
///
/// Inbound: a deployment bearer or a logs-only telemetry capability.
pub async fn ingest_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    ingest(state, headers, body, TelemetrySignal::Logs).await
}

/// POST /v1/traces
///
/// Inbound: deployment bearer. See [`ingest_logs`] doc for the auth model.
pub async fn ingest_traces(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    ingest(state, headers, body, TelemetrySignal::Traces).await
}

/// POST /v1/metrics
///
/// Inbound: deployment bearer. See [`ingest_logs`] doc for the auth model.
pub async fn ingest_metrics(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    ingest(state, headers, body, TelemetrySignal::Metrics).await
}

async fn ingest(
    state: AppState,
    headers: HeaderMap,
    body: Bytes,
    signal: TelemetrySignal,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    if !state.authz.can_ingest_telemetry(&subject, signal) {
        return ErrorData::forbidden("Cannot ingest this telemetry signal").into_response();
    }

    let caller = match &subject.scope {
        Scope::Deployment {
            deployment_id,
            project_id,
        } => TelemetryCaller {
            deployment_id: Some(deployment_id.clone()),
            project_id: Some(project_id.clone()),
            workspace_id: Some(subject.workspace_id.clone()),
            gateway_log_source: None,
        },
        Scope::Telemetry {
            project_id,
            capability: TelemetryCapability::GatewayLogs { source },
        } if subject.role == Role::TelemetryCapability => TelemetryCaller {
            deployment_id: None,
            project_id: Some(project_id.clone()),
            workspace_id: Some(subject.workspace_id.clone()),
            gateway_log_source: Some(*source),
        },
        _ => {
            return ErrorData::forbidden("Telemetry subject has an unsupported scope")
                .into_response();
        }
    };

    if signal == TelemetrySignal::Metrics && !state.telemetry_backend.supports_metrics() {
        return ErrorData::internal("Metrics backend is not configured").into_response();
    }
    let body = match decode_body(&headers, body) {
        Ok(body) => body,
        Err(response) => return response,
    };
    match state.telemetry_backend.ingest(signal, &caller, body).await {
        Ok(()) => ([("content-type", "application/x-protobuf")], Bytes::new()).into_response(),
        Err(e) => {
            if signal == TelemetrySignal::Metrics {
                if let Some(loss) = alien_client_core::otlp_retention::RetentionLoss::from_error(&e)
                {
                    return retention_response(loss);
                }
            }
            e.into_response()
        }
    }
}

pub(super) const MAX_OTLP_BYTES: usize = 4 * 1024 * 1024;

fn decode_body(headers: &HeaderMap, body: Bytes) -> Result<Bytes, Response> {
    use axum::http::StatusCode;
    use std::io::Read;
    if body.len() > MAX_OTLP_BYTES {
        return Err(StatusCode::PAYLOAD_TOO_LARGE.into_response());
    }
    let encoding = headers
        .get("content-encoding")
        .map(|value| {
            value
                .to_str()
                .map(|value| value.trim().to_ascii_lowercase())
        })
        .transpose()
        .map_err(|_| StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response())?;
    match encoding.as_deref() {
        None | Some("identity") => Ok(body),
        Some("gzip") => {
            let mut decoded = Vec::new();
            flate2::read::MultiGzDecoder::new(body.as_ref())
                .take((MAX_OTLP_BYTES + 1) as u64)
                .read_to_end(&mut decoded)
                .map_err(|_| StatusCode::BAD_REQUEST.into_response())?;
            if decoded.len() > MAX_OTLP_BYTES {
                return Err(StatusCode::PAYLOAD_TOO_LARGE.into_response());
            }
            Ok(decoded.into())
        }
        Some(_) => Err(StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response()),
    }
}

fn retention_response(loss: alien_client_core::otlp_retention::RetentionLoss) -> Response {
    use opentelemetry_proto::tonic::collector::metrics::v1::{
        ExportMetricsPartialSuccess, ExportMetricsServiceResponse,
    };
    use prost::Message;
    let mut response = if loss.fresh_accepted {
        let rejected = i64::try_from(loss.rejected_count).unwrap_or(i64::MAX);
        (
            [("content-type", "application/x-protobuf")],
            ExportMetricsServiceResponse {
                partial_success: Some(ExportMetricsPartialSuccess {
                    rejected_data_points: rejected,
                    error_message: "retention-expired".into(),
                }),
            }
            .encode_to_vec(),
        )
            .into_response()
    } else {
        (
            axum::http::StatusCode::GONE,
            Json(serde_json::json!({"code":"OTLP_RETENTION_LOSS","retryable":false})),
        )
            .into_response()
    };
    let headers = response.headers_mut();
    if let Some(receipt) = loss.accepted_receipt {
        if let Ok(value) = receipt.parse() {
            headers.insert("x-otlp-accepted-receipt", value);
        }
    }
    headers.insert(
        "x-otlp-rejection-reason",
        "retention-expired".parse().unwrap(),
    );
    headers.insert("x-otlp-loss-receipt", loss.loss_receipt.parse().unwrap());
    headers.insert(
        "x-otlp-retention-cutoff-nanos",
        loss.cutoff_nanos.to_string().parse().unwrap(),
    );
    headers.insert(
        "x-otlp-rejected-count",
        loss.rejected_count.to_string().parse().unwrap(),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn gzip_is_decoded_and_expanded_size_is_bounded() {
        let encode = |bytes: &[u8]| {
            let mut gzip =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            gzip.write_all(bytes).unwrap();
            Bytes::from(gzip.finish().unwrap())
        };
        let headers = HeaderMap::from_iter([(
            axum::http::header::CONTENT_ENCODING,
            "gzip".parse().unwrap(),
        )]);
        assert_eq!(
            decode_body(&headers, encode(b"protobuf bytes")).unwrap(),
            &b"protobuf bytes"[..]
        );
        assert_eq!(
            decode_body(&headers, encode(&vec![0; MAX_OTLP_BYTES + 1]))
                .unwrap_err()
                .status(),
            axum::http::StatusCode::PAYLOAD_TOO_LARGE
        );
        assert_eq!(
            decode_body(&headers, Bytes::from_static(b"broken"))
                .unwrap_err()
                .status(),
            axum::http::StatusCode::BAD_REQUEST
        );
    }
    #[tokio::test]
    async fn relay_preserves_standard_partial_success_and_both_durable_receipts() {
        use prost::Message;
        let response = retention_response(alien_client_core::otlp_retention::RetentionLoss {
            loss_receipt: "a".repeat(64),
            cutoff_nanos: 123,
            rejected_count: 2,
            fresh_accepted: true,
            accepted_receipt: Some("fresh-receipt".into()),
        });
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let headers = response.headers().clone();
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let decoded = opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceResponse::decode(body).unwrap();
        assert!(
            alien_client_core::otlp_retention::RetentionLoss::from_response(
                reqwest::StatusCode::OK,
                &headers,
                decoded.partial_success.map(|p| p.rejected_data_points)
            )
            .unwrap()
            .fresh_accepted
        );
    }
}
