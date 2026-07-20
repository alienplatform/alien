use std::path::Path;

use super::*;
use crate::config::{AwsConfig, AzureConfig, GcpConfig};

pub(super) fn upsert_env(env: &mut Vec<(String, String)>, key: &str, value: String) {
    if let Some((_, existing)) = env.iter_mut().find(|(existing, _)| existing == key) {
        *existing = value;
    } else {
        env.push((key.to_string(), value));
    }
}

pub(super) fn aws_env(config: &AwsConfig) -> Vec<(String, String)> {
    let mut env = vec![
        (
            "AWS_ACCESS_KEY_ID".to_string(),
            config.access_key_id.clone(),
        ),
        (
            "AWS_SECRET_ACCESS_KEY".to_string(),
            config.secret_access_key.clone(),
        ),
        ("AWS_REGION".to_string(), config.region.clone()),
        ("AWS_DEFAULT_REGION".to_string(), config.region.clone()),
    ];
    if let Some(token) = &config.session_token {
        env.push(("AWS_SESSION_TOKEN".to_string(), token.clone()));
    }
    env
}

pub(super) fn terraform_env(
    config: &TestConfig,
    platform: Platform,
) -> anyhow::Result<Vec<(String, String)>> {
    match platform {
        Platform::Aws => Ok(aws_env(
            config.aws_target.as_ref().context("AWS target missing")?,
        )),
        Platform::Gcp => gcp_env(config.gcp_target.as_ref().context("GCP target missing")?),
        Platform::Azure => Ok(azure_env(
            config
                .azure_target
                .as_ref()
                .context("Azure target missing")?,
        )),
        _ => Ok(Vec::new()),
    }
}

fn gcp_env(config: &GcpConfig) -> anyhow::Result<Vec<(String, String)>> {
    let mut env = vec![
        ("GOOGLE_PROJECT".to_string(), config.project_id.clone()),
        ("GOOGLE_REGION".to_string(), config.region.clone()),
    ];
    if let Some(path) = gke_auth_plugin_path_env() {
        env.push(("PATH".to_string(), path));
    }
    if let Some(credentials) = &config.credentials_json {
        let file = tempfile::NamedTempFile::new()
            .context("Failed to create temporary GCP credentials file")?;
        std::fs::write(file.path(), credentials)?;
        let (_file, path) = file.keep()?;
        env.push((
            "GOOGLE_APPLICATION_CREDENTIALS".to_string(),
            path.display().to_string(),
        ));
    } else if let Ok(path) = std::env::var("GOOGLE_APPLICATION_CREDENTIALS") {
        if !path.trim().is_empty() {
            env.push(("GOOGLE_APPLICATION_CREDENTIALS".to_string(), path));
        }
    }
    Ok(env)
}

fn gke_auth_plugin_path_env() -> Option<String> {
    if std::process::Command::new("gke-gcloud-auth-plugin")
        .arg("--version")
        .output()
        .is_ok()
    {
        return None;
    }

    let output = std::process::Command::new("gcloud")
        .args(["info", "--format=value(installation.sdk_root)"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sdk_root = String::from_utf8(output.stdout).ok()?;
    let sdk_root = sdk_root.trim();
    if sdk_root.is_empty() {
        return None;
    }
    let plugin_dir = Path::new(sdk_root).join("bin");
    if !plugin_dir.join("gke-gcloud-auth-plugin").exists() {
        return None;
    }
    let existing = std::env::var("PATH").unwrap_or_default();
    Some(format!("{}:{existing}", plugin_dir.display()))
}

fn azure_env(config: &AzureConfig) -> Vec<(String, String)> {
    vec![
        (
            "ARM_SUBSCRIPTION_ID".to_string(),
            config.subscription_id.clone(),
        ),
        ("ARM_TENANT_ID".to_string(), config.tenant_id.clone()),
        ("ARM_CLIENT_ID".to_string(), config.client_id.clone()),
        (
            "ARM_CLIENT_SECRET".to_string(),
            config.client_secret.clone(),
        ),
    ]
}
