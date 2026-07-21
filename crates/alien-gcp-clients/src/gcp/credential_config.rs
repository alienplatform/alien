use std::collections::HashMap;

use alien_client_core::{ErrorData, Result};
use alien_error::AlienError;

use super::{GcpClientConfig, GcpClientConfigExt, GcpCredentials};

pub(super) async fn parse_credentials_json(
    credential_data: &serde_json::Value,
    raw_json: &str,
    environment_variables: &HashMap<String, String>,
) -> Result<(GcpCredentials, String, String)> {
    let cred_type = credential_data["type"]
        .as_str()
        .unwrap_or("service_account");

    if cred_type == "external_account" {
        let audience = credential_data["audience"]
            .as_str()
            .ok_or_else(|| {
                AlienError::new(ErrorData::InvalidClientConfig {
                    message: "audience not found in external_account credentials".to_string(),
                    errors: None,
                })
            })?
            .to_string();

        let subject_token_type = credential_data["subject_token_type"]
            .as_str()
            .unwrap_or("urn:ietf:params:oauth:token-type:jwt")
            .to_string();

        let token_url = credential_data["token_url"]
            .as_str()
            .unwrap_or("https://sts.googleapis.com/v1/token")
            .to_string();

        let credential_source_file = credential_data["credential_source"]["file"]
            .as_str()
            .ok_or_else(|| {
                AlienError::new(ErrorData::InvalidClientConfig {
                    message: "credential_source.file not found in external_account credentials"
                        .to_string(),
                    errors: None,
                })
            })?
            .to_string();

        let service_account_impersonation_url = credential_data
            ["service_account_impersonation_url"]
            .as_str()
            .map(|value| value.to_string());

        let project_id = environment_variables
            .get("GCP_PROJECT_ID")
            .or_else(|| environment_variables.get("GOOGLE_CLOUD_PROJECT"))
            .cloned()
            .or_else(|| {
                credential_data["quota_project_id"]
                    .as_str()
                    .map(|value| value.to_string())
            })
            .ok_or_else(|| {
                AlienError::new(ErrorData::InvalidClientConfig {
                    message: "Missing GCP_PROJECT_ID or GOOGLE_CLOUD_PROJECT environment variable for external_account credentials".to_string(),
                    errors: None,
                })
            })?;

        let region = environment_variables
            .get("GCP_REGION")
            .ok_or_else(|| {
                AlienError::new(ErrorData::InvalidClientConfig {
                    message:
                        "Missing GCP_REGION environment variable for external_account credentials"
                            .to_string(),
                    errors: None,
                })
            })?
            .clone();

        Ok((
            GcpCredentials::ExternalAccount {
                audience,
                subject_token_type,
                token_url,
                credential_source_file,
                service_account_impersonation_url,
            },
            project_id,
            region,
        ))
    } else if cred_type == "authorized_user" {
        let client_id = credential_data["client_id"]
            .as_str()
            .ok_or_else(|| {
                AlienError::new(ErrorData::InvalidClientConfig {
                    message: "client_id not found in authorized_user credentials".to_string(),
                    errors: None,
                })
            })?
            .to_string();

        let client_secret = credential_data["client_secret"]
            .as_str()
            .ok_or_else(|| {
                AlienError::new(ErrorData::InvalidClientConfig {
                    message: "client_secret not found in authorized_user credentials".to_string(),
                    errors: None,
                })
            })?
            .to_string();

        let refresh_token = credential_data["refresh_token"]
            .as_str()
            .ok_or_else(|| {
                AlienError::new(ErrorData::InvalidClientConfig {
                    message: "refresh_token not found in authorized_user credentials".to_string(),
                    errors: None,
                })
            })?
            .to_string();

        // authorized_user credentials don't contain project_id, so we need it from
        // the environment or from quota_project_id in the file
        let project_id = environment_variables.get("GCP_PROJECT_ID")
            .cloned()
            .or_else(|| credential_data["quota_project_id"].as_str().map(|s| s.to_string()))
            .ok_or_else(|| AlienError::new(ErrorData::InvalidClientConfig {
                message: "Missing GCP_PROJECT_ID environment variable for authorized_user credentials \
                          (quota_project_id not found in credentials file either)".to_string(),
                errors: None,
            }))?;

        let region = environment_variables
            .get("GCP_REGION")
            .ok_or_else(|| {
                AlienError::new(ErrorData::InvalidClientConfig {
                    message:
                        "Missing GCP_REGION environment variable for authorized_user credentials"
                            .to_string(),
                    errors: None,
                })
            })?
            .clone();

        Ok((
            GcpCredentials::AuthorizedUser {
                client_id,
                client_secret,
                refresh_token,
            },
            project_id,
            region,
        ))
    } else {
        // service_account or other types — treat as service account key
        let project_id = credential_data["project_id"]
            .as_str()
            .ok_or_else(|| {
                AlienError::new(ErrorData::InvalidClientConfig {
                    message: "project_id not found in credentials file".to_string(),
                    errors: None,
                })
            })?
            .to_string();

        let region = if let Some(region) = environment_variables.get("GCP_REGION") {
            region.clone()
        } else {
            GcpClientConfig::fetch_metadata_region().await?
        };

        Ok((
            GcpCredentials::ServiceAccountKey {
                json: raw_json.to_string(),
            },
            project_id,
            region,
        ))
    }
}

pub(super) fn read_well_known_adc_file() -> Option<(String, serde_json::Value)> {
    let home = std::env::var("HOME").ok()?;
    let adc_path = std::path::Path::new(&home)
        .join(".config")
        .join("gcloud")
        .join("application_default_credentials.json");

    let json = std::fs::read_to_string(&adc_path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&json).ok()?;
    Some((json, value))
}
