//! The verifier's GCP calls: Parameter Manager for the signing settings and
//! ledger, Secret Manager for deployer inputs, Cloud Run services and
//! managed instance groups, with the token of the service account the
//! verifier runs as.

use std::collections::BTreeMap;

use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use base64::Engine as _;
use serde_json::{json, Value};

use alien_signing::settings::names;
use alien_signing::{Ledger, Lookup, Workload};

use crate::cloud_run::ServiceStore;
use crate::cloud_write::{CloudWrite, Facts};
use crate::error::{ErrorData, Result};
use crate::function::StillNewest;
use crate::machines;
use crate::plan::{FunctionSpec, Place};
use crate::trust::InstallStore;
use crate::verifier::Backend;

const METADATA_TOKEN: &str =
    "http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token";

/// Where the verifier's token comes from.
#[derive(Debug, Clone)]
pub enum TokenSource {
    /// The metadata server of the Cloud Run instance the verifier runs on.
    Metadata,
    /// A file holding an access token, for running the verifier elsewhere.
    File(String),
}

/// GCP REST calls for one project and region.
#[derive(Debug, Clone)]
pub struct Gcp {
    http: reqwest::Client,
    tokens: TokenSource,
    /// Project id.
    pub project: String,
    /// Cloud Run region.
    pub region: String,
}

fn failed(operation: impl Into<String>) -> ErrorData {
    ErrorData::CloudCallFailed {
        operation: operation.into(),
    }
}

impl Gcp {
    /// A client for `project` and `region`.
    pub fn new(tokens: TokenSource, project: String, region: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            tokens,
            project,
            region,
        }
    }

    /// The same client, sending its calls through `http` (a workstation
    /// run behind a TLS proxy passes one that trusts the proxy's CA).
    pub fn with_http(self, http: reqwest::Client) -> Self {
        Self { http, ..self }
    }

    async fn token(&self) -> Result<String> {
        match &self.tokens {
            TokenSource::File(path) => std::fs::read_to_string(path)
                .map(|token| token.trim().to_string())
                .into_alien_error()
                .context(failed(format!("reading the token file {path}"))),
            TokenSource::Metadata => {
                let response: Value = self
                    .http
                    .get(METADATA_TOKEN)
                    .header("Metadata-Flavor", "Google")
                    .send()
                    .await
                    .into_alien_error()
                    .context(failed("metadata token"))?
                    .json()
                    .await
                    .into_alien_error()
                    .context(failed("metadata token"))?;
                response["access_token"]
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| AlienError::new(failed("metadata token has no access_token")))
            }
        }
    }

    /// Send a request; `Ok(None)` for 404.
    async fn call(
        &self,
        method: reqwest::Method,
        url: &str,
        body: Option<&Value>,
        operation: &str,
    ) -> Result<Option<Value>> {
        let mut request = self
            .http
            .request(method, url)
            .bearer_auth(self.token().await?);
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request
            .send()
            .await
            .into_alien_error()
            .context(failed(operation))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .into_alien_error()
            .context(failed(operation))?;
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(AlienError::new(failed(format!(
                "{operation}: HTTP {status}: {}",
                text.chars().take(2000).collect::<String>()
            ))));
        }
        if text.trim().is_empty() {
            return Ok(Some(Value::Null));
        }
        serde_json::from_str(&text)
            .map(Some)
            .into_alien_error()
            .context(failed(format!("{operation}: the response is not JSON")))
    }

    fn service_url(&self, name: &str) -> String {
        format!(
            "https://run.googleapis.com/v2/projects/{}/locations/{}/services/{name}",
            self.project, self.region
        )
    }

    /// The latest version of a secret, or `None` if it has none.
    pub async fn secret(&self, secret: &str) -> Result<Option<String>> {
        let url = format!(
            "https://secretmanager.googleapis.com/v1/projects/{}/secrets/{secret}/versions/latest:access",
            self.project
        );
        let Some(response) = self
            .call(
                reqwest::Method::GET,
                &url,
                None,
                &format!("secretmanager access {secret}"),
            )
            .await?
        else {
            return Ok(None);
        };
        let data = response["payload"]["data"].as_str().ok_or_else(|| {
            AlienError::new(failed(format!("secret {secret} carries no payload")))
        })?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .into_alien_error()
            .context(failed(format!("secret {secret} is not base64")))?;
        String::from_utf8(bytes)
            .map(Some)
            .into_alien_error()
            .context(failed(format!("secret {secret} is not UTF-8")))
    }

    /// Add a version to a secret.
    pub async fn add_secret_version(&self, secret: &str, value: &str) -> Result<()> {
        let url = format!(
            "https://secretmanager.googleapis.com/v1/projects/{}/secrets/{secret}:addVersion",
            self.project
        );
        let body = json!({"payload": {
            "data": base64::engine::general_purpose::STANDARD.encode(value),
        }});
        self.call(
            reqwest::Method::POST,
            &url,
            Some(&body),
            &format!("secretmanager addVersion {secret}"),
        )
        .await?
        .ok_or_else(|| AlienError::new(failed(format!("secret {secret} does not exist"))))?;
        Ok(())
    }

    /// Read `url`: what it holds, absent on 404, denied on 403.
    async fn lookup(&self, url: &str, operation: &str) -> Result<Option<Value>> {
        let response = self
            .http
            .get(url)
            .bearer_auth(self.token().await?)
            .send()
            .await
            .into_alien_error()
            .context(failed(operation))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .into_alien_error()
            .context(failed(operation))?;
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if status == reqwest::StatusCode::FORBIDDEN {
            return Err(AlienError::new(ErrorData::Refused {
                reason: format!("{operation}: denied"),
            }));
        }
        if !status.is_success() {
            return Err(AlienError::new(failed(format!(
                "{operation}: HTTP {status}: {}",
                text.chars().take(2000).collect::<String>()
            ))));
        }
        serde_json::from_str(&text)
            .map(Some)
            .into_alien_error()
            .context(failed(format!("{operation}: the response is not JSON")))
    }

    fn parameter_url(&self, parameter: &str) -> String {
        format!(
            "https://parametermanager.googleapis.com/v1/projects/{}/locations/global/parameters/{parameter}",
            self.project
        )
    }

    /// The newest version of Parameter Manager parameter `parameter`.
    pub async fn parameter(&self, parameter: &str) -> Result<Lookup> {
        let mut versions = Vec::new();
        let mut page: Option<String> = None;
        loop {
            let mut url = format!("{}/versions?pageSize=100", self.parameter_url(parameter));
            if let Some(token) = &page {
                url.push_str(&format!("&pageToken={token}"));
            }
            let listed = match self
                .lookup(&url, &format!("parametermanager list {parameter}"))
                .await
            {
                Ok(Some(listed)) => listed,
                Ok(None) => return Ok(Lookup::Absent),
                Err(error) if matches!(error.error, Some(ErrorData::Refused { .. })) => {
                    return Ok(Lookup::Denied)
                }
                Err(error) => return Err(error),
            };
            versions.extend(
                listed["parameterVersions"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default(),
            );
            match listed["nextPageToken"].as_str() {
                Some(token) if !token.is_empty() => page = Some(token.to_string()),
                _ => break,
            }
        }
        let Some(name) = newest_version(parameter, &versions)? else {
            return Ok(Lookup::Absent);
        };
        let url = format!("https://parametermanager.googleapis.com/v1/{name}?view=FULL");
        let version = self
            .lookup(&url, &format!("parametermanager get {name}"))
            .await?
            .ok_or_else(|| AlienError::new(failed(format!("version {name} disappeared"))))?;
        let data = version["payload"]["data"]
            .as_str()
            .ok_or_else(|| AlienError::new(failed(format!("version {name} carries no payload"))))?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .into_alien_error()
            .context(failed(format!("parameter {parameter} is not base64")))?;
        Ok(Lookup::Found(bytes))
    }

    /// Add a version holding `value` to parameter `parameter`.
    pub async fn add_parameter_version(&self, parameter: &str, value: &str) -> Result<()> {
        let id = format!("v{}", chrono::Utc::now().timestamp_millis());
        let url = format!(
            "{}/versions?parameterVersionId={id}",
            self.parameter_url(parameter)
        );
        let body = json!({"payload": {
            "data": base64::engine::general_purpose::STANDARD.encode(value),
        }});
        self.call(
            reqwest::Method::POST,
            &url,
            Some(&body),
            &format!("parametermanager create version {parameter}"),
        )
        .await?
        .ok_or_else(|| AlienError::new(failed(format!("parameter {parameter} does not exist"))))?;
        Ok(())
    }

    /// Resize managed instance group `name`, zonal when `zone` is given,
    /// and wait for the operation, so a resize Compute Engine fails is an
    /// error and the answer's size is the group's.
    pub async fn resize_group(&self, name: &str, zone: Option<&str>, size: u32) -> Result<()> {
        let url = resize_url(&self.project, &self.region, name, zone, size)?;
        let what = format!("compute instanceGroupManagers.resize {name}");
        let operation = self
            .call(reqwest::Method::POST, url.as_str(), None, &what)
            .await?
            .ok_or_else(|| {
                AlienError::new(failed(format!("instance group {name} does not exist")))
            })?;
        self.wait_compute(operation, &what).await
    }
}

/// The `instanceGroupManagers.resize` URL for MIG `name`: zonal in `zone`,
/// which must be a zone of `region`, or regional in `region`.
fn resize_url(
    project: &str,
    region: &str,
    name: &str,
    zone: Option<&str>,
    size: u32,
) -> Result<reqwest::Url> {
    let (scope, place) = match zone {
        Some(zone) => {
            machines::check_zone(zone, region)?;
            ("zones", zone)
        }
        None => ("regions", region),
    };
    let mut url = machines::segments_url(
        "https://compute.googleapis.com/compute/v1",
        &[
            "projects",
            project,
            scope,
            place,
            "instanceGroupManagers",
            name,
            "resize",
        ],
    )?;
    url.query_pairs_mut().append_pair("size", &size.to_string());
    Ok(url)
}

/// The name of the newest of `versions`, or `None` when there are none.
/// The newest being disabled is an error, never a reason to read an older
/// one: disabling the latest ledger would otherwise bring back an older one.
fn newest_version(parameter: &str, versions: &[Value]) -> Result<Option<String>> {
    let mut newest: Option<(chrono::DateTime<chrono::FixedOffset>, &str, bool)> = None;
    for version in versions {
        let created = version["createTime"]
            .as_str()
            .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok());
        let (Some(created), Some(name)) = (created, version["name"].as_str()) else {
            return Err(AlienError::new(failed(format!(
                "parameter {parameter} lists a version without a name or create time"
            ))));
        };
        let disabled = version["disabled"].as_bool().unwrap_or(false);
        if newest.is_none_or(|(newest_created, ..)| created > newest_created) {
            newest = Some((created, name, disabled));
        }
    }
    match newest {
        None => Ok(None),
        Some((_, name, true)) => Err(AlienError::new(ErrorData::Refused {
            reason: format!(
                "the newest version of parameter {parameter}, {name}, is disabled; an administrator must enable it or add a new one"
            ),
        })),
        Some((_, name, false)) => Ok(Some(name.to_string())),
    }
}

const COMPUTE: &str = "https://compute.googleapis.com/compute/v1/";

impl Gcp {
    /// Wait for Compute Engine operation `operation` to finish; its error,
    /// if it has one, is an error here.
    async fn wait_compute(&self, operation: Value, what: &str) -> Result<()> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(300);
        let mut operation = operation;
        loop {
            if operation["status"].as_str() == Some("DONE") {
                if let Some(error) = operation.get("error") {
                    return Err(AlienError::new(failed(format!("{what}: {error}"))));
                }
                return Ok(());
            }
            if tokio::time::Instant::now() > deadline {
                return Err(AlienError::new(failed(format!(
                    "{what}: still running after 300 s"
                ))));
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let link = operation["selfLink"]
                .as_str()
                .ok_or_else(|| {
                    AlienError::new(failed(format!("{what}: the operation has no selfLink")))
                })?
                .to_string();
            operation = self
                .call(reqwest::Method::GET, &link, None, what)
                .await?
                .ok_or_else(|| {
                    AlienError::new(failed(format!("{what}: the operation disappeared")))
                })?;
        }
    }

    async fn compute_write(&self, method: reqwest::Method, path: &str, body: &Value) -> Result<()> {
        let what = format!("compute {method} {path}");
        let operation = self
            .call(method, &format!("{COMPUTE}{path}"), Some(body), &what)
            .await?
            .ok_or_else(|| AlienError::new(failed(format!("{what}: not found"))))?;
        self.wait_compute(operation, &what).await
    }
}

#[async_trait]
impl crate::gcp_machines::GcpMachineApi for Gcp {
    async fn get(&self, path: &str) -> Result<Option<Value>> {
        self.call(
            reqwest::Method::GET,
            &format!("{COMPUTE}{path}"),
            None,
            &format!("compute get {path}"),
        )
        .await
    }

    async fn template_names(&self, pattern: &str) -> Result<Vec<String>> {
        let mut names = Vec::new();
        let mut page: Option<String> = None;
        loop {
            let mut url = machines::segments_url(
                COMPUTE.trim_end_matches('/'),
                &["projects", &self.project, "global", "instanceTemplates"],
            )?;
            url.query_pairs_mut()
                .append_pair("filter", &format!("name eq \"{pattern}\""));
            if let Some(token) = &page {
                url.query_pairs_mut().append_pair("pageToken", token);
            }
            let listed = self
                .call(
                    reqwest::Method::GET,
                    url.as_str(),
                    None,
                    "compute instanceTemplates.list",
                )
                .await?
                .ok_or_else(|| {
                    AlienError::new(failed("compute instanceTemplates.list: not found"))
                })?;
            names.extend(
                listed["items"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|item| item["name"].as_str().map(str::to_string)),
            );
            match listed["nextPageToken"].as_str() {
                Some(token) if !token.is_empty() => page = Some(token.to_string()),
                _ => return Ok(names),
            }
        }
    }

    async fn insert(&self, path: &str, body: &Value) -> Result<()> {
        self.compute_write(reqwest::Method::POST, path, body).await
    }

    async fn patch(&self, path: &str, body: &Value) -> Result<()> {
        self.compute_write(reqwest::Method::PATCH, path, body).await
    }

    async fn members(&self, path: &str) -> Result<Vec<String>> {
        let what = format!("compute listManagedInstances {path}");
        let mut members = Vec::new();
        let mut page: Option<String> = None;
        loop {
            let mut url = reqwest::Url::parse(&format!("{COMPUTE}{path}/listManagedInstances"))
                .into_alien_error()
                .context(failed(what.clone()))?;
            if let Some(token) = &page {
                url.query_pairs_mut().append_pair("pageToken", token);
            }
            let listed = self
                .call(reqwest::Method::POST, url.as_str(), Some(&json!({})), &what)
                .await?
                .ok_or_else(|| AlienError::new(failed(format!("{what}: not found"))))?;
            members.extend(
                listed["managedInstances"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|member| member["instance"].as_str().map(str::to_string)),
            );
            match listed["nextPageToken"].as_str() {
                Some(token) if !token.is_empty() => page = Some(token.to_string()),
                _ => return Ok(members),
            }
        }
    }
}

/// The install's Parameter Manager parameters and Secret Manager inputs.
pub struct GcpInstall(pub Gcp);

#[async_trait]
impl InstallStore for GcpInstall {
    async fn settings(&self, prefix: &str) -> Result<Lookup> {
        self.0.parameter(&names::gcp_settings(prefix)).await
    }

    async fn ledger(&self, prefix: &str) -> Result<Lookup> {
        self.0.parameter(&names::gcp_ledger(prefix)).await
    }

    async fn store_ledger(&self, prefix: &str, ledger: &Ledger) -> Result<()> {
        self.0
            .add_parameter_version(&names::gcp_ledger(prefix), &ledger.to_json())
            .await
    }

    async fn input(&self, prefix: &str, name: &str) -> Result<Option<String>> {
        self.0.secret(&names::gcp_input(prefix, name)).await
    }

    fn settings_name(&self, prefix: &str) -> String {
        names::gcp_settings(prefix)
    }

    fn ledger_name(&self, prefix: &str) -> String {
        names::gcp_ledger(prefix)
    }
}

/// Cloud Run services and machine groups, and the machine images this
/// build boots.
pub struct GcpBackend(pub Gcp, pub crate::machines::ImagePins);

#[async_trait]
impl Backend for GcpBackend {
    async fn binding_reads(
        &self,
        _place: &Place,
        _workload: &Workload,
    ) -> Result<BTreeMap<String, String>> {
        Ok(BTreeMap::new())
    }

    fn check_write(&self, facts: &Facts<'_>, write: &CloudWrite) -> Result<()> {
        crate::cloud_write::cloud_run::check_write(facts, write)
    }

    async fn forward(
        &self,
        facts: &Facts<'_>,
        write: &CloudWrite,
        newest: &dyn StillNewest,
    ) -> Result<Value> {
        crate::cloud_write::cloud_run::forward(&self.0, facts, write, newest).await
    }

    async fn apply(
        &self,
        _place: &Place,
        spec: &FunctionSpec,
        newest: &dyn StillNewest,
    ) -> Result<()> {
        if spec.architecture != "x86_64" {
            return Err(AlienError::new(ErrorData::Refused {
                reason: "Cloud Run runs only x86_64".to_string(),
            }));
        }
        // One verifier instance serves the install, so nothing writes between this check and
        // the write.
        newest.check().await?;
        crate::cloud_run::apply(&self.0, spec).await.map(|_| ())
    }

    async fn check(&self, spec: &FunctionSpec) -> Result<()> {
        let back = ServiceStore::get(&self.0, &spec.name).await?;
        crate::cloud_run::check_read_back(spec, &back)
    }

    async fn resize(&self, target: &str, zone: Option<&str>, size: u32) -> Result<()> {
        self.0.resize_group(target, zone, size).await
    }

    async fn remove(
        &self,
        _place: &Place,
        target: &str,
        zone: Option<&str>,
        machines: &[String],
        minimum: u32,
    ) -> Result<(Vec<String>, u32)> {
        crate::gcp_machines::remove(
            &self.0,
            &self.0.project,
            &self.0.region,
            target,
            zone,
            machines,
            minimum,
        )
        .await
    }

    async fn roll(
        &self,
        place: &Place,
        release: &alien_signing::ReleaseBody,
        request: &crate::machines::RollMachines,
    ) -> Result<Value> {
        crate::gcp_machines::roll(
            &self.0,
            place,
            &self.0.project,
            &self.0.region,
            release,
            request,
            &self.1,
        )
        .await
    }
}

/// The project the verifier runs in and its region, from the metadata server.
pub async fn metadata_identity() -> Result<(String, String)> {
    let http = reqwest::Client::new();
    let get = |path: &'static str| {
        let http = http.clone();
        async move {
            http.get(format!(
                "http://metadata.google.internal/computeMetadata/v1/{path}"
            ))
            .header("Metadata-Flavor", "Google")
            .send()
            .await
            .into_alien_error()
            .context(failed(format!("metadata {path}")))?
            .text()
            .await
            .into_alien_error()
            .context(failed(format!("metadata {path}")))
        }
    };
    let project = get("project/project-id").await?;
    // `projects/<number>/regions/<region>`.
    let region = get("instance/region").await?;
    let region = match region.trim().rsplit_once("/regions/") {
        Some((_, region)) if !region.is_empty() && !region.contains('/') => region.to_string(),
        _ => {
            return Err(AlienError::new(failed(format!(
                "metadata instance/region '{region}' is not projects/<number>/regions/<region>"
            ))))
        }
    };
    let project = project.trim();
    if project.is_empty() {
        return Err(AlienError::new(failed(
            "metadata project/project-id is empty",
        )));
    }
    Ok((project.to_string(), region))
}

#[async_trait]
impl ServiceStore for Gcp {
    async fn get(&self, name: &str) -> Result<Value> {
        self.call(
            reqwest::Method::GET,
            &self.service_url(name),
            None,
            &format!("run.services.get {name}"),
        )
        .await?
        .ok_or_else(|| {
            AlienError::new(ErrorData::Refused {
                reason: format!("service {name} does not exist; setup creates it"),
            })
        })
    }

    async fn find(&self, name: &str) -> Result<Option<Value>> {
        self.call(
            reqwest::Method::GET,
            &self.service_url(name),
            None,
            &format!("run.services.get {name}"),
        )
        .await
    }

    async fn create(&self, name: &str, service: &Value) -> Result<()> {
        let url = format!(
            "https://run.googleapis.com/v2/projects/{}/locations/{}/services?serviceId={name}",
            self.project, self.region
        );
        self.call(
            reqwest::Method::POST,
            &url,
            Some(service),
            &format!("run.services.create {name}"),
        )
        .await?
        .ok_or_else(|| {
            AlienError::new(failed(format!(
                "run.services.create {name}: the location does not exist"
            )))
        })?;
        Ok(())
    }

    async fn policy(&self, name: &str) -> Result<Value> {
        self.call(
            reqwest::Method::GET,
            &format!("{}:getIamPolicy", self.service_url(name)),
            None,
            &format!("run.services.getIamPolicy {name}"),
        )
        .await?
        .ok_or_else(|| AlienError::new(failed(format!("service {name} does not exist"))))
    }

    async fn set_policy(&self, name: &str, policy: &Value) -> Result<()> {
        self.call(
            reqwest::Method::POST,
            &format!("{}:setIamPolicy", self.service_url(name)),
            Some(&json!({ "policy": policy })),
            &format!("run.services.setIamPolicy {name}"),
        )
        .await?
        .ok_or_else(|| AlienError::new(failed(format!("service {name} does not exist"))))?;
        Ok(())
    }

    async fn replace(&self, name: &str, service: &Value) -> Result<()> {
        self.call(
            reqwest::Method::PATCH,
            &self.service_url(name),
            Some(service),
            &format!("run.services.patch {name}"),
        )
        .await?
        .ok_or_else(|| {
            AlienError::new(ErrorData::Refused {
                reason: format!("service {name} does not exist; setup creates it"),
            })
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(name: &str, created: &str, disabled: bool) -> Value {
        json!({"name": name, "createTime": created, "disabled": disabled})
    }

    #[test]
    fn the_newest_parameter_version_is_read_and_a_disabled_one_refuses() {
        let older = version("p/versions/v1", "2026-11-01T00:00:00Z", false);
        let newer = version("p/versions/v2", "2026-11-02T00:00:00Z", false);
        assert_eq!(
            newest_version("p", &[newer.clone(), older.clone()]).unwrap(),
            Some("p/versions/v2".to_string())
        );
        assert_eq!(newest_version("p", &[]).unwrap(), None);

        // Attack: disable the newest ledger so an older one is read.
        let disabled = version("p/versions/v2", "2026-11-02T00:00:00Z", true);
        let error = newest_version("p", &[older.clone(), disabled.clone()]).unwrap_err();
        assert_eq!(
            error.message,
            "Refused: the newest version of parameter p, p/versions/v2, is disabled; an administrator must enable it or add a new one"
        );
        // Every version disabled is the same error, not an absent parameter.
        let both = [
            version("p/versions/v1", "2026-11-01T00:00:00Z", true),
            disabled,
        ];
        assert!(newest_version("p", &both).is_err());
        // A version the listing doesn't describe is an error too.
        assert!(newest_version("p", &[json!({"name": "p/versions/v3"})]).is_err());
        // Create times compare as times, whatever their precision.
        let fractional = version("p/versions/v3", "2026-11-02T00:00:00.5Z", false);
        assert_eq!(
            newest_version("p", &[fractional, newer]).unwrap(),
            Some("p/versions/v3".to_string())
        );
    }

    #[test]
    fn a_zonal_resize_goes_to_a_zone_of_the_verifiers_region() {
        let url = resize_url(
            "shop-prod",
            "us-central1",
            "acme-prod-compute-general",
            Some("us-central1-a"),
            4,
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "https://compute.googleapis.com/compute/v1/projects/shop-prod/zones/us-central1-a/instanceGroupManagers/acme-prod-compute-general/resize?size=4"
        );
        let url = resize_url(
            "shop-prod",
            "us-central1",
            "acme-prod-compute-general",
            None,
            4,
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "https://compute.googleapis.com/compute/v1/projects/shop-prod/regions/us-central1/instanceGroupManagers/acme-prod-compute-general/resize?size=4"
        );
    }

    #[test]
    fn a_zone_that_is_not_one_of_the_verifiers_region_is_refused() {
        for zone in [
            // Attack: the zone carries the rest of a URL with its own size.
            "us-central1-a/instanceGroupManagers/acme-prod-compute-big/resize?size=100&",
            "us-central1-a?size=100",
            "../../regions/us-central1",
            "europe-west1-b",
            "us-central1",
            "us-central1-ab",
            "US-CENTRAL1-A",
        ] {
            let error = resize_url(
                "shop-prod",
                "us-central1",
                "acme-prod-compute-general",
                Some(zone),
                4,
            )
            .expect_err(zone);
            assert_eq!(
                error.message,
                format!("Refused: '{zone}' is not a zone of the verifier's region us-central1"),
            );
        }
    }
}
