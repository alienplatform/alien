//! Audience-bound proof generation using refreshable cloud credentials.

use std::time::{Duration, SystemTime};

use alien_client_core::{runtime_credentials::RuntimeProofProvider, ErrorData, Result};
use alien_error::AlienError;
use async_trait::async_trait;
use aws_sigv4::{
    http_request::{sign, SignableBody, SignableRequest, SignatureLocation, SigningSettings},
    sign::v4,
};
use serde_json::{json, Value};

use aws_credential_types::Credentials;

use crate::AwsCredentialProvider;

/// Generates short-acceptance STS proofs. Credential refresh remains with the
/// existing cloud provider; no deployment credential is used on failure.
pub struct AwsRuntimeProof {
    credentials: AwsCredentialProvider,
    region: String,
}

impl AwsRuntimeProof {
    pub fn new(credentials: AwsCredentialProvider, region: String) -> Result<Self> {
        if region.is_empty()
            || !region
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(failed());
        }
        Ok(Self {
            credentials,
            region,
        })
    }
}

#[async_trait]
impl RuntimeProofProvider for AwsRuntimeProof {
    async fn proof(&self, audience: &str) -> Result<Value> {
        self.credentials
            .ensure_fresh()
            .await
            .map_err(|_| failed())?;
        let presigned_url = presign_caller_identity(
            &self.credentials.get_credentials(),
            &self.region,
            audience,
            SystemTime::now(),
            Duration::from_secs(300),
        )?;
        Ok(json!({"type":"aws", "presignedUrl": presigned_url}))
    }
}

/// Presign a regional STS GetCallerIdentity request whose signature covers the
/// `x-alien-manager-audience` header. Only temporary role credentials qualify.
pub fn presign_caller_identity(
    credentials: &Credentials,
    region: &str,
    audience: &str,
    signed_at: SystemTime,
    expires_in: Duration,
) -> Result<String> {
    if credentials.session_token().is_none() {
        return Err(failed());
    }
    let identity = credentials.clone().into();
    let mut settings = SigningSettings::default();
    settings.signature_location = SignatureLocation::QueryParams;
    settings.expires_in = Some(expires_in);
    let params = v4::SigningParams::builder()
        .identity(&identity)
        .region(region)
        .name("sts")
        .time(signed_at)
        .settings(settings)
        .build()
        .map_err(|_| failed())?
        .into();
    let uri =
        format!("https://sts.{region}.amazonaws.com/?Action=GetCallerIdentity&Version=2011-06-15");
    let request = SignableRequest::new(
        "GET",
        &uri,
        [("x-alien-manager-audience", audience)].into_iter(),
        SignableBody::Bytes(&[]),
    )
    .map_err(|_| failed())?;
    let (instructions, _) = sign(request, &params).map_err(|_| failed())?.into_parts();
    let mut request = http::Request::builder()
        .method("GET")
        .uri(&uri)
        .header("x-alien-manager-audience", audience)
        .body(())
        .map_err(|_| failed())?;
    instructions.apply_to_request_http1x(&mut request);
    Ok(request.uri().to_string())
}

fn failed() -> alien_client_core::Error {
    AlienError::new(ErrorData::AuthenticationError {
        message: "Cannot obtain an audience-bound cloud proof".into(),
    })
}
