//! Deployment-owned OTLP destinations, resolved from trusted outputs and secret snapshots.
use std::time::Duration;

use alien_core::{
    EnvironmentVariable, EnvironmentVariableType, OtlpDestinationAuthentication,
    OtlpDestinationEndpoint, OtlpSignalDestination, Stack, StackSettings, StackState,
    TelemetryDestinations,
};
use alien_error::{AlienError, Context, GenericError, IntoAlienError};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use opentelemetry_proto::tonic::collector::{
    logs::v1::ExportLogsServiceResponse, metrics::v1::ExportMetricsServiceResponse,
    trace::v1::ExportTraceServiceResponse,
};
use prost::Message;
use sha2::Sha256;

#[derive(Clone, Copy)]
pub enum Signal {
    Metrics,
    Traces,
    Logs,
}

/// Contains credential material; deliberately does not implement Debug or Serialize.
#[derive(Clone)]
pub struct ResolvedDestination {
    endpoint: reqwest::Url,
    authentication: OtlpDestinationAuthentication,
    secret: String,
    deployment_id: String,
}

pub fn destinations<'a>(
    settings: &'a StackSettings,
    stack: Option<&'a Stack>,
) -> Option<&'a TelemetryDestinations> {
    settings
        .telemetry_destinations
        .as_ref()
        .or_else(|| stack.and_then(|stack| stack.telemetry_destinations.as_ref()))
}

pub fn resolve(
    destination: &OtlpSignalDestination,
    stack_state: Option<&StackState>,
    variables: &[EnvironmentVariable],
    deployment_id: &str,
) -> Result<ResolvedDestination, AlienError> {
    if deployment_id.is_empty() || deployment_id.contains(['/', '?', '#', '%']) {
        return Err(failure("Invalid telemetry deployment identity"));
    }
    let endpoint = match &destination.endpoint {
        OtlpDestinationEndpoint::Url { url } => reqwest::Url::parse(&expand(url, deployment_id)),
        OtlpDestinationEndpoint::Resource {
            resource_id,
            public_endpoint_name,
            path,
        } => {
            if !path.starts_with('/')
                || path.contains(['?', '#'])
                || path.split('/').any(|part| part == "..")
            {
                return Err(failure("Invalid telemetry endpoint path"));
            }
            let resource = stack_state
                .and_then(|state| state.resources.get(resource_id))
                .ok_or_else(|| failure("Telemetry destination resource is not deployed"))?;
            if resource.status != alien_core::ResourceStatus::Running {
                return Err(failure("Telemetry destination resource is not ready"));
            }
            let outputs = resource
                .outputs
                .as_ref()
                .ok_or_else(|| failure("Telemetry destination has no outputs"))?;
            let value = serde_json::to_value(outputs)
                .into_alien_error()
                .context(GenericError {
                    message: "Could not resolve telemetry endpoint outputs".into(),
                })?;
            let base = value
                .get("publicEndpoints")
                .and_then(|endpoints| endpoints.get(public_endpoint_name))
                .and_then(|endpoint| endpoint.get("url"))
                .and_then(|value| value.as_str())
                .ok_or_else(|| failure("Declared telemetry public endpoint is unavailable"))?;
            let mut url = reqwest::Url::parse(base)
                .into_alien_error()
                .context(GenericError {
                    message: "Invalid declared telemetry endpoint".into(),
                })?;
            // set_path never replaces the authority, including for a path beginning with //.
            url.set_path(&format!(
                "{}{}",
                url.path().trim_end_matches('/'),
                expand(path, deployment_id)
            ));
            Ok(url)
        }
    }
    .into_alien_error()
    .context(GenericError {
        message: "Invalid telemetry destination URL".into(),
    })?;
    let allowed_scheme = endpoint.scheme() == "https"
        || (destination.allow_insecure_http && endpoint.scheme() == "http");
    if !allowed_scheme
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
    {
        return Err(failure("Telemetry destination must use an approved HTTP authority without credentials, query, or fragment"));
    }
    let host = endpoint
        .host_str()
        .ok_or_else(|| failure("Telemetry destination hostname is required"))?;
    if matches!(
        host,
        "metadata.google.internal"
            | "metadata.google"
            | "169.254.169.254"
            | "169.254.170.2"
            | "168.63.129.16"
            | "100.100.100.200"
    ) {
        return Err(failure("Cloud metadata cannot be a telemetry destination"));
    }
    if let Ok(ip) = host.trim_matches(['[', ']']).parse::<std::net::IpAddr>() {
        let denied = match ip {
            std::net::IpAddr::V4(ip) => {
                ip.is_link_local() || ip.is_unspecified() || ip.is_multicast()
            }
            std::net::IpAddr::V6(ip) => {
                matches!(ip.to_string().as_str(), "fd00:ec2::254" | "fd20:ce::254")
                    || ip.is_unicast_link_local()
                    || ip.is_unspecified()
                    || ip.is_multicast()
                    || ip.to_ipv4_mapped().is_some_and(|ip| ip.is_link_local())
            }
        };
        if denied {
            return Err(failure("Invalid telemetry destination address"));
        }
    }
    let reference = match &destination.authentication {
        OtlpDestinationAuthentication::Bearer {
            token_environment_variable,
        } => token_environment_variable,
        OtlpDestinationAuthentication::JwtHs256 {
            key_environment_variable,
            issuer,
            audience,
            claims,
        } => {
            if issuer.is_empty()
                || audience.is_empty()
                || ["iss", "aud", "iat", "exp", "nbf"]
                    .iter()
                    .any(|key| claims.contains_key(*key))
            {
                return Err(failure(
                    "JWT destination cannot override standard issuer, audience, or time claims",
                ));
            }
            key_environment_variable
        }
    };
    let mut matches = variables
        .iter()
        .filter(|variable| variable.name == *reference);
    let variable = matches
        .next()
        .filter(|variable| {
            variable.var_type == EnvironmentVariableType::Secret && !variable.value.is_empty()
        })
        .ok_or_else(|| {
            failure("Telemetry destination requires a resolved secret environment variable")
        })?;
    if matches.next().is_some() {
        return Err(failure(
            "Telemetry secret environment reference is ambiguous",
        ));
    }
    if matches!(
        &destination.authentication,
        OtlpDestinationAuthentication::JwtHs256 { .. }
    ) && variable.value.len() < 32
    {
        return Err(failure("Telemetry HMAC key must contain at least 32 bytes"));
    }
    Ok(ResolvedDestination {
        endpoint,
        authentication: destination.authentication.clone(),
        secret: variable.value.clone(),
        deployment_id: deployment_id.into(),
    })
}

impl ResolvedDestination {
    fn token(&self) -> Result<String, AlienError> {
        match &self.authentication {
            OtlpDestinationAuthentication::Bearer { .. } => Ok(self.secret.clone()),
            OtlpDestinationAuthentication::JwtHs256 {
                issuer,
                audience,
                claims,
                ..
            } => {
                let mut claims = claims.clone();
                for value in claims.values_mut() {
                    expand_claim(value, &self.deployment_id);
                }
                let now = chrono::Utc::now().timestamp();
                claims.insert("iss".into(), issuer.clone().into());
                claims.insert("aud".into(), audience.clone().into());
                claims.insert("iat".into(), now.into());
                claims.insert("exp".into(), (now + 300).into());
                let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"HS256","typ":"JWT"}"#);
                let payload = URL_SAFE_NO_PAD.encode(
                    serde_json::to_vec(&claims)
                        .into_alien_error()
                        .context(GenericError {
                            message: "Could not encode destination JWT".into(),
                        })?,
                );
                let content = format!("{header}.{payload}");
                let mut mac = Hmac::<Sha256>::new_from_slice(self.secret.as_bytes())
                    .map_err(|_| failure("Invalid telemetry signing key"))?;
                mac.update(content.as_bytes());
                Ok(format!(
                    "{content}.{}",
                    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
                ))
            }
        }
    }

    pub async fn export(&self, signal: Signal, bytes: Vec<u8>) -> Result<(), AlienError> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .build()
            .into_alien_error()
            .context(GenericError {
                message: "Could not initialize OTLP destination client".into(),
            })?;
        let mut response = client
            .post(self.endpoint.clone())
            .bearer_auth(self.token()?)
            .header("content-type", "application/x-protobuf")
            .body(bytes)
            .send()
            .await
            .into_alien_error()
            .context(GenericError {
                message: "Declared OTLP destination export failed".into(),
            })?;
        let status = response.status();
        let headers = response.headers().clone();
        if matches!(signal, Signal::Metrics) {
            if let Some(loss) = alien_client_core::otlp_retention::RetentionLoss::from_response(
                status, &headers, None,
            ) {
                return Err(loss.into_error());
            }
        }
        if !status.is_success() {
            return Err(failure(
                "Declared OTLP destination did not acknowledge export",
            ));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .into_alien_error()
            .context(GenericError {
                message: "Could not read declared OTLP acknowledgement".into(),
            })?
        {
            if bytes.len() + chunk.len() > 64 * 1024 {
                return Err(failure("Declared OTLP acknowledgement exceeds size limit"));
            }
            bytes.extend_from_slice(&chunk);
        }
        if matches!(signal, Signal::Metrics) {
            let acknowledgement = ExportMetricsServiceResponse::decode(bytes.as_slice())
                .into_alien_error()
                .context(GenericError {
                    message: "Invalid declared OTLP acknowledgement".into(),
                })?;
            if let Some(loss) = alien_client_core::otlp_retention::RetentionLoss::from_response(
                status,
                &headers,
                acknowledgement
                    .partial_success
                    .as_ref()
                    .map(|p| p.rejected_data_points),
            ) {
                return Err(loss.into_error());
            }
        }
        let rejected = match signal {
            Signal::Metrics => {
                ExportMetricsServiceResponse::decode(bytes.as_slice()).map(|value| {
                    value.partial_success.is_some_and(|partial| {
                        partial.rejected_data_points > 0 || !partial.error_message.is_empty()
                    })
                })
            }
            Signal::Traces => ExportTraceServiceResponse::decode(bytes.as_slice()).map(|value| {
                value.partial_success.is_some_and(|partial| {
                    partial.rejected_spans > 0 || !partial.error_message.is_empty()
                })
            }),
            Signal::Logs => ExportLogsServiceResponse::decode(bytes.as_slice()).map(|value| {
                value.partial_success.is_some_and(|partial| {
                    partial.rejected_log_records > 0 || !partial.error_message.is_empty()
                })
            }),
        }
        .into_alien_error()
        .context(GenericError {
            message: "Invalid declared OTLP acknowledgement".into(),
        })?;
        if rejected {
            return Err(failure(
                "Declared OTLP destination partially rejected export",
            ));
        }
        Ok(())
    }
}

fn expand(value: &str, deployment_id: &str) -> String {
    value.replace("${deploymentId}", deployment_id)
}
fn expand_claim(value: &mut serde_json::Value, deployment_id: &str) {
    match value {
        serde_json::Value::String(text) => *text = expand(text, deployment_id),
        serde_json::Value::Array(values) => {
            for value in values {
                expand_claim(value, deployment_id);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values_mut() {
                expand_claim(value, deployment_id);
            }
        }
        _ => {}
    }
}
fn failure(message: &str) -> AlienError {
    AlienError::new(GenericError {
        message: message.into(),
    })
}

pub fn for_deployment(
    state: &alien_core::DeploymentState,
    config: &alien_core::DeploymentConfig,
    signal: Signal,
    deployment_id: &str,
) -> Result<Option<ResolvedDestination>, AlienError> {
    let stack = state
        .runtime_metadata
        .as_ref()
        .and_then(|metadata| metadata.prepared_stack.as_ref())
        .or_else(|| state.current_release.as_ref().map(|release| &release.stack))
        .or_else(|| state.target_release.as_ref().map(|release| &release.stack));
    let Some(destinations) = destinations(&config.stack_settings, stack) else {
        return Ok(None);
    };
    let selected = match signal {
        Signal::Metrics => &destinations.metrics,
        Signal::Traces => &destinations.traces,
        Signal::Logs => &destinations.logs,
    };
    selected
        .as_ref()
        .map(|destination| {
            resolve(
                destination,
                state.stack_state.as_ref(),
                &config.environment_variables.variables,
                deployment_id,
            )
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn secret() -> EnvironmentVariable {
        EnvironmentVariable {
            name: "STORE_APPEND_KEY".into(),
            value: "test-append-key-at-least-32-bytes-long".into(),
            var_type: EnvironmentVariableType::Secret,
            target_resources: Some(vec!["store".into()]),
        }
    }
    fn destination() -> OtlpSignalDestination {
        OtlpSignalDestination {
            endpoint: OtlpDestinationEndpoint::Url {
                url: "https://store.example/v2/databases/${deploymentId}/tables/metrics/metrics"
                    .into(),
            },
            authentication: OtlpDestinationAuthentication::JwtHs256 {
                key_environment_variable: "STORE_APPEND_KEY".into(),
                issuer: "store-instance".into(),
                audience: "append-only".into(),
                claims: BTreeMap::from([
                    ("databaseId".into(), "${deploymentId}".into()),
                    ("actions".into(), serde_json::json!(["append"])),
                ]),
            },
            allow_insecure_http: false,
        }
    }
    #[test]
    fn mints_bounded_jwt_from_targeted_secret_without_exposing_key_in_configuration() {
        let destination = resolve(&destination(), None, &[secret()], "deployment-1").unwrap();
        assert_eq!(
            destination.endpoint.path(),
            "/v2/databases/deployment-1/tables/metrics/metrics"
        );
        let token = destination.token().unwrap();
        let parts: Vec<_> = token.split('.').collect();
        let mut verifier = Hmac::<Sha256>::new_from_slice(secret().value.as_bytes()).unwrap();
        verifier.update(format!("{}.{}", parts[0], parts[1]).as_bytes());
        verifier
            .verify_slice(&URL_SAFE_NO_PAD.decode(parts[2]).unwrap())
            .unwrap();
        let claims: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(claims["databaseId"], "deployment-1");
        assert_eq!(claims["aud"], "append-only");
        assert_eq!(claims["actions"], serde_json::json!(["append"]));
        assert_eq!(
            claims["exp"].as_i64().unwrap() - claims["iat"].as_i64().unwrap(),
            300
        );
    }
    #[test]
    fn rejects_reserved_claim_overrides_plaintext_secrets_and_metadata_destinations() {
        let mut config = destination();
        if let OtlpDestinationAuthentication::JwtHs256 { claims, .. } = &mut config.authentication {
            claims.insert("exp".into(), 9999999999u64.into());
        }
        assert!(resolve(&config, None, &[secret()], "deployment").is_err());
        let mut plain = secret();
        plain.var_type = EnvironmentVariableType::Plain;
        assert!(resolve(&destination(), None, &[plain], "deployment").is_err());
        let mut metadata = destination();
        metadata.allow_insecure_http = true;
        metadata.endpoint = OtlpDestinationEndpoint::Url {
            url: "http://169.254.169.254/latest/meta-data".into(),
        };
        assert!(resolve(&metadata, None, &[secret()], "deployment").is_err());
    }
    #[tokio::test]
    async fn upstream_partial_ack_is_a_failure_without_redirect_or_fallback() {
        let server = httpmock::MockServer::start_async().await;
        let mock = server.mock_async(|when, then| {
            when.method(httpmock::Method::POST).path("/metrics");
            then.status(200).body(ExportMetricsServiceResponse { partial_success: Some(opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsPartialSuccess { rejected_data_points: 1, error_message: "quota".into() }) }.encode_to_vec());
        }).await;
        let mut config = destination();
        config.allow_insecure_http = true;
        config.endpoint = OtlpDestinationEndpoint::Url {
            url: server.url("/metrics"),
        };
        let destination = resolve(&config, None, &[secret()], "deployment").unwrap();
        assert!(destination
            .export(Signal::Metrics, Vec::new())
            .await
            .is_err());
        mock.assert_hits_async(1).await;
    }
    #[tokio::test]
    async fn durable_partial_retention_ack_is_preserved_as_a_typed_loss() {
        let server = httpmock::MockServer::start_async().await;
        let receipt = "b".repeat(64);
        server.mock_async(|when, then| {
            when.method(httpmock::Method::POST).path("/metrics");
            then.status(200)
                .header("x-otlp-rejection-reason", "retention-expired")
                .header("x-otlp-loss-receipt", &receipt)
                .header("x-otlp-retention-cutoff-nanos", "123")
                .header("x-otlp-rejected-count", "2")
                .header("x-otlp-accepted-receipt", "fresh-durable-receipt")
                .body(ExportMetricsServiceResponse { partial_success: Some(opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsPartialSuccess { rejected_data_points: 2, error_message: "retention-expired".into() }) }.encode_to_vec());
        }).await;
        let mut config = destination();
        config.allow_insecure_http = true;
        config.endpoint = OtlpDestinationEndpoint::Url {
            url: server.url("/metrics"),
        };
        let destination = resolve(&config, None, &[secret()], "deployment").unwrap();
        let error = destination
            .export(Signal::Metrics, Vec::new())
            .await
            .unwrap_err();
        let loss = alien_client_core::otlp_retention::RetentionLoss::from_error(&error).unwrap();
        assert!(loss.fresh_accepted);
        assert_eq!(loss.rejected_count, 2);
        assert_eq!(loss.loss_receipt, receipt);
    }
}
