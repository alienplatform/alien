//! Local metadata and protected parent-session proof sources.

use std::{path::PathBuf, time::Duration};

use alien_error::AlienError;
use async_trait::async_trait;
use reqwest::{redirect::Policy, Client};
use serde_json::{json, Value};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use tokio::io::AsyncReadExt;

use crate::{runtime_credentials::RuntimeProofProvider, ErrorData, Result};

/// GCP metadata identity token. The endpoint is fixed by the runtime, never
/// chosen by an exchange caller.
pub struct GcpMetadataProof;

#[async_trait]
impl RuntimeProofProvider for GcpMetadataProof {
    async fn proof(&self, audience: &str) -> Result<Value> {
        let client = Client::builder()
            .redirect(Policy::none())
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|_| failed("cannot initialize metadata transport"))?;
        let mut response = client
            .get("http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/identity")
            .header("Metadata-Flavor", "Google")
            .query(&[("audience", audience), ("format", "full")])
            .send()
            .await
            .map_err(|_| failed("metadata request failed"))?;
        if !response.status().is_success() {
            return Err(failed("metadata identity request refused"));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| failed("metadata response failed"))?
        {
            if body.len() + chunk.len() > 32 * 1024 {
                return Err(failed("metadata response exceeds limit"));
            }
            body.extend_from_slice(&chunk);
        }
        let token =
            String::from_utf8(body).map_err(|_| failed("invalid metadata identity response"))?;
        if token.trim().is_empty() {
            return Err(failed("empty metadata identity response"));
        }
        Ok(json!({"type":"gcp", "idToken": token}))
    }
}

/// Reads a parent session on every exchange. The session issuer owns renewal;
/// this source cannot extend the parent's lifetime.
pub struct SessionFileProof(pub PathBuf);

#[async_trait]
impl RuntimeProofProvider for SessionFileProof {
    async fn proof(&self, _audience: &str) -> Result<Value> {
        let file = tokio::fs::File::open(&self.0)
            .await
            .map_err(|_| failed("cannot open parent session file"))?;
        let metadata = file
            .metadata()
            .await
            .map_err(|_| failed("cannot inspect parent session file"))?;
        if !metadata.is_file() || metadata.len() > 16 * 1024 {
            return Err(failed("invalid parent session file"));
        }
        #[cfg(unix)]
        {
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err(failed("parent session file must be private"));
            }
        }
        let mut token = String::new();
        file.take(16 * 1024 + 1)
            .read_to_string(&mut token)
            .await
            .map_err(|_| failed("cannot read parent session file"))?;
        if token.len() > 16 * 1024 {
            return Err(failed("parent session exceeds limit"));
        }
        let token = token.trim();
        if token.is_empty() || token.starts_with("ax_cap_") {
            return Err(failed("invalid parent session"));
        }
        Ok(json!({"type":"session", "token":token}))
    }
}

fn failed(message: &str) -> crate::Error {
    AlienError::new(ErrorData::AuthenticationError {
        message: message.into(),
    })
}
