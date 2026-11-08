//! The verifier's Azure calls: the signing settings tag on the install's
//! resource group, the ledger and deployer inputs in the install's Key
//! Vault, Container Apps and scale sets, with the token of the managed
//! identity the verifier runs as.

use std::collections::BTreeMap;

use alien_azure_clients::azure::container_apps::CONTAINER_APPS_API_VERSION;
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use serde_json::{json, Value};

use alien_signing::settings::names;
use alien_signing::{Ledger, Lookup, Workload};

use crate::cloud_write::{CloudWrite, Facts};
use crate::container_apps::AppStore;
use crate::error::{ErrorData, Result};
use crate::function::StillNewest;
use crate::plan::Place;
use crate::trust::InstallStore;
use crate::verifier::Backend;

const ARM: &str = "https://management.azure.com";
const KEY_VAULT_API: &str = "7.4";

fn failed(operation: impl Into<String>) -> ErrorData {
    ErrorData::CloudCallFailed {
        operation: operation.into(),
    }
}

/// Azure REST calls for one resource group and Key Vault.
#[derive(Debug, Clone)]
pub struct Azure {
    http: reqwest::Client,
    /// `/subscriptions/<id>/resourceGroups/<name>`, where the apps live.
    pub resource_group: String,
    /// `https://<vault>.vault.azure.net`.
    pub vault: String,
    /// Client id of the verifier's user-assigned identity, if it has several.
    pub client_id: Option<String>,
    /// Resource id of the Container Apps environment the install's workloads run in, as setup
    /// names it. None when the install has none.
    pub environment: Option<String>,
}

impl Azure {
    /// A client for the apps in `resource_group` and secrets in `vault`.
    pub fn new(resource_group: String, vault: String, client_id: Option<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            resource_group,
            vault: vault.trim_end_matches('/').to_string(),
            client_id,
            environment: None,
        }
    }

    /// The same client, creating workloads in Container Apps environment `environment`.
    pub fn with_environment(self, environment: Option<String>) -> Self {
        Self {
            environment,
            ..self
        }
    }

    /// The same client, sending its calls through `http` (a workstation
    /// run behind a TLS proxy passes one that trusts the proxy's CA).
    pub fn with_http(self, http: reqwest::Client) -> Self {
        Self { http, ..self }
    }

    /// A token for `resource` from the Container Apps identity endpoint.
    async fn token(&self, resource: &str) -> Result<String> {
        let endpoint = std::env::var("IDENTITY_ENDPOINT")
            .map_err(|_| AlienError::new(failed("IDENTITY_ENDPOINT is not set")))?;
        let header = std::env::var("IDENTITY_HEADER")
            .map_err(|_| AlienError::new(failed("IDENTITY_HEADER is not set")))?;
        let mut query = vec![("resource", resource), ("api-version", "2019-08-01")];
        if let Some(client_id) = &self.client_id {
            query.push(("client_id", client_id));
        }
        let response: Value = self
            .http
            .get(&endpoint)
            .query(&query)
            .header("X-IDENTITY-HEADER", header)
            .send()
            .await
            .into_alien_error()
            .context(failed("managed identity token"))?
            .json()
            .await
            .into_alien_error()
            .context(failed("managed identity token"))?;
        response["access_token"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| AlienError::new(failed("the identity endpoint returned no token")))
    }

    /// Send a request; `Ok(None)` for 404.
    async fn call(
        &self,
        method: reqwest::Method,
        url: &str,
        resource: &str,
        body: Option<&Value>,
        operation: &str,
    ) -> Result<Option<Value>> {
        let mut request = self
            .http
            .request(method, url)
            .bearer_auth(self.token(resource).await?);
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

    fn app_url(&self, name: &str) -> String {
        format!(
            "{ARM}{}/providers/Microsoft.App/containerApps/{name}?api-version={CONTAINER_APPS_API_VERSION}",
            self.resource_group
        )
    }

    /// The value of a Key Vault secret, or `None` if it doesn't exist.
    pub async fn secret(&self, name: &str) -> Result<Option<String>> {
        let url = format!("{}/secrets/{name}?api-version={KEY_VAULT_API}", self.vault);
        Ok(self
            .call(
                reqwest::Method::GET,
                &url,
                "https://vault.azure.net",
                None,
                &format!("keyvault get {name}"),
            )
            .await?
            .and_then(|secret| secret["value"].as_str().map(str::to_string)))
    }

    /// Set a Key Vault secret.
    pub async fn set_secret(&self, name: &str, value: &str) -> Result<()> {
        let url = format!("{}/secrets/{name}?api-version={KEY_VAULT_API}", self.vault);
        self.call(
            reqwest::Method::PUT,
            &url,
            "https://vault.azure.net",
            Some(&json!({"value": value})),
            &format!("keyvault set {name}"),
        )
        .await?;
        Ok(())
    }

    /// The value of tag `name` on the install's resource group: absent when
    /// the group has no such tag, denied when the verifier may not read it.
    pub async fn resource_group_tag(&self, name: &str) -> Result<Lookup> {
        let url = format!("{ARM}{}?api-version=2021-04-01", self.resource_group);
        let response = self
            .http
            .get(&url)
            .bearer_auth(self.token(ARM).await?)
            .send()
            .await
            .into_alien_error()
            .context(failed("resourceGroups get"))?;
        let status = response.status();
        if status == reqwest::StatusCode::FORBIDDEN {
            return Ok(Lookup::Denied);
        }
        if !status.is_success() {
            return Err(AlienError::new(failed(format!(
                "resourceGroups get: HTTP {status}"
            ))));
        }
        let group: Value = response
            .json()
            .await
            .into_alien_error()
            .context(failed("resourceGroups get"))?;
        Ok(match group["tags"][name].as_str() {
            Some(value) => Lookup::Found(value.as_bytes().to_vec()),
            None => Lookup::Absent,
        })
    }

    /// Set the capacity of scale set `name` and wait until it is
    /// provisioned, so a resize Azure fails is an error.
    pub async fn resize_scale_set(&self, name: &str, size: u32) -> Result<()> {
        let url = scale_set_url(&self.resource_group, name)?;
        let what = format!("virtualMachineScaleSets update {name}");
        self.call(
            reqwest::Method::PATCH,
            url.as_str(),
            ARM,
            Some(&json!({"sku": {"capacity": size}})),
            &what,
        )
        .await?
        .ok_or_else(|| {
            AlienError::new(ErrorData::Refused {
                reason: format!("scale set {name} does not exist"),
            })
        })?;
        self.provisioned(url.as_str(), &what).await.map(|_| ())
    }

    /// Read `url` until its provisioning state is Succeeded; Failed,
    /// Canceled or ten minutes without it are errors.
    async fn provisioned(&self, url: &str, what: &str) -> Result<Value> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(600);
        loop {
            let current = self
                .call(reqwest::Method::GET, url, ARM, None, what)
                .await?
                .ok_or_else(|| AlienError::new(failed(format!("{what}: gone after the write"))))?;
            match current["properties"]["provisioningState"].as_str() {
                Some("Succeeded") => return Ok(current),
                Some("Failed") | Some("Canceled") => {
                    return Err(AlienError::new(failed(format!(
                        "{what}: provisioning {}",
                        current["properties"]["provisioningState"]
                    ))))
                }
                _ if tokio::time::Instant::now() > deadline => {
                    return Err(AlienError::new(failed(format!(
                        "{what}: not provisioned after 600 s"
                    ))))
                }
                _ => tokio::time::sleep(std::time::Duration::from_secs(5)).await,
            }
        }
    }
}

/// The ARM URL of scale set `name` in `resource_group`.
fn scale_set_url(resource_group: &str, name: &str) -> Result<reqwest::Url> {
    let mut url = crate::machines::segments_url(
        &format!("{ARM}{resource_group}"),
        &[
            "providers",
            "Microsoft.Compute",
            "virtualMachineScaleSets",
            name,
        ],
    )?;
    url.query_pairs_mut()
        .append_pair("api-version", "2024-07-01");
    Ok(url)
}

/// The install's resource-group tag and Key Vault secrets.
pub struct AzureInstall(pub Azure);

#[async_trait]
impl InstallStore for AzureInstall {
    /// The resource group holds one settings tag. It belongs to `prefix`
    /// only if it names `prefix`; otherwise this place is empty.
    async fn settings(&self, prefix: &str) -> Result<Lookup> {
        let lookup = self.0.resource_group_tag(names::AZURE_SETTINGS_TAG).await?;
        if let Lookup::Found(bytes) = &lookup {
            let named: Value =
                serde_json::from_slice(bytes)
                    .into_alien_error()
                    .context(ErrorData::Refused {
                        reason: format!(
                            "the {} tag does not hold signing settings",
                            names::AZURE_SETTINGS_TAG
                        ),
                    })?;
            if named["prefix"].as_str() != Some(prefix) {
                return Ok(Lookup::Absent);
            }
        }
        Ok(lookup)
    }

    async fn ledger(&self, _prefix: &str) -> Result<Lookup> {
        Ok(match self.0.secret(names::AZURE_LEDGER_SECRET).await? {
            Some(value) => Lookup::Found(value.into_bytes()),
            None => Lookup::Absent,
        })
    }

    async fn store_ledger(&self, _prefix: &str, ledger: &Ledger) -> Result<()> {
        self.0
            .set_secret(names::AZURE_LEDGER_SECRET, &ledger.to_json())
            .await
    }

    async fn input(&self, _prefix: &str, name: &str) -> Result<Option<String>> {
        self.0.secret(&format!("input-{name}")).await
    }

    fn settings_name(&self, _prefix: &str) -> String {
        format!("resource group tag {}", names::AZURE_SETTINGS_TAG)
    }

    fn ledger_name(&self, _prefix: &str) -> String {
        format!("Key Vault secret {}", names::AZURE_LEDGER_SECRET)
    }
}

#[async_trait]
impl crate::azure_machines::AzureMachineApi for Azure {
    async fn get(&self, path: &str) -> Result<Option<Value>> {
        self.call(
            reqwest::Method::GET,
            &format!("{ARM}{path}"),
            ARM,
            None,
            &format!("ARM get {path}"),
        )
        .await
    }

    async fn post(&self, path: &str, body: &Value) -> Result<()> {
        let what = format!("ARM post {path}");
        self.call(
            reqwest::Method::POST,
            &format!("{ARM}{path}"),
            ARM,
            Some(body),
            &what,
        )
        .await?
        .ok_or_else(|| AlienError::new(failed(format!("{what}: not found"))))?;
        Ok(())
    }

    async fn put(&self, path: &str, body: &Value) -> Result<Value> {
        let what = format!("ARM put {path}");
        self.call(
            reqwest::Method::PUT,
            &format!("{ARM}{path}"),
            ARM,
            Some(body),
            &what,
        )
        .await?
        .ok_or_else(|| AlienError::new(failed(format!("{what}: not found"))))?;
        self.provisioned(&format!("{ARM}{path}"), &what).await
    }
}

/// Container Apps and scale sets, and the machine images this build boots.
pub struct AzureBackend(pub Azure, pub crate::machines::ImagePins);

#[async_trait]
impl Backend for AzureBackend {
    async fn binding_reads(
        &self,
        _place: &Place,
        _workload: &Workload,
    ) -> Result<BTreeMap<String, String>> {
        Ok(BTreeMap::new())
    }

    fn check_write(&self, facts: &Facts<'_>, write: &CloudWrite) -> Result<()> {
        crate::cloud_write::container_apps::check_write(facts, write, self.0.environment.as_deref())
    }

    async fn forward(
        &self,
        facts: &Facts<'_>,
        write: &CloudWrite,
        newest: &dyn StillNewest,
    ) -> Result<Value> {
        crate::cloud_write::container_apps::forward(&self.0, facts, write, newest).await
    }

    async fn resize(&self, target: &str, zone: Option<&str>, size: u32) -> Result<()> {
        if let Some(zone) = zone {
            return Err(AlienError::new(ErrorData::Refused {
                reason: format!("a scale set is resized as a whole, not in zone '{zone}'"),
            }));
        }
        self.0.resize_scale_set(target, size).await
    }

    async fn remove(
        &self,
        place: &Place,
        target: &str,
        zone: Option<&str>,
        machines: &[String],
        minimum: u32,
    ) -> Result<(Vec<String>, u32)> {
        if zone.is_some() {
            return Err(AlienError::new(ErrorData::Refused {
                reason: "a scale set takes no zone".to_string(),
            }));
        }
        let alien_signing::InstallId::Azure {
            subscription,
            resource_group,
        } = &place.install
        else {
            return Err(AlienError::new(ErrorData::Refused {
                reason: "this is the Azure removal".to_string(),
            }));
        };
        crate::azure_machines::remove(
            &self.0,
            subscription,
            resource_group,
            target,
            machines,
            minimum,
        )
        .await
    }

    /// Azure finishes the delete in the background; the controller waits for the app to be
    /// gone.
    async fn delete_workload(&self, name: &str) -> Result<bool> {
        let deleted = self
            .0
            .call(
                reqwest::Method::DELETE,
                &self.0.app_url(name),
                ARM,
                None,
                &format!("containerApps delete {name}"),
            )
            .await?;
        Ok(deleted.is_some())
    }

    /// Deleting a scale set takes its machines with it; Azure finishes it in the background, so
    /// the caller asks again until the scale set is gone.
    async fn delete_machines(&self, target: &str, zone: Option<&str>) -> Result<bool> {
        if let Some(zone) = zone {
            return Err(AlienError::new(ErrorData::Refused {
                reason: format!("a scale set takes no zone here ({zone})"),
            }));
        }
        let url = scale_set_url(&self.0.resource_group, target)?;
        let Some(scale_set) = self
            .0
            .call(
                reqwest::Method::GET,
                url.as_str(),
                ARM,
                None,
                &format!("virtualMachineScaleSets get {target}"),
            )
            .await?
        else {
            return Ok(true);
        };
        if scale_set["properties"]["provisioningState"].as_str() != Some("Deleting") {
            self.0
                .call(
                    reqwest::Method::DELETE,
                    url.as_str(),
                    ARM,
                    None,
                    &format!("virtualMachineScaleSets delete {target}"),
                )
                .await?;
        }
        Ok(false)
    }

    async fn roll(
        &self,
        place: &Place,
        release: &alien_signing::ReleaseBody,
        request: &crate::machines::RollMachines,
    ) -> Result<Value> {
        let alien_signing::InstallId::Azure {
            subscription,
            resource_group,
        } = &place.install
        else {
            return Err(AlienError::new(ErrorData::Refused {
                reason: "this is the Azure roll".to_string(),
            }));
        };
        crate::azure_machines::roll(
            &self.0,
            place,
            subscription,
            resource_group,
            release,
            request,
            &self.1,
        )
        .await
    }
}

#[async_trait]
impl AppStore for Azure {
    async fn get(&self, name: &str) -> Result<Value> {
        self.call(
            reqwest::Method::GET,
            &self.app_url(name),
            ARM,
            None,
            &format!("containerApps get {name}"),
        )
        .await?
        .ok_or_else(|| {
            AlienError::new(ErrorData::Refused {
                reason: format!("container app {name} does not exist; setup creates it"),
            })
        })
    }

    async fn create(&self, name: &str, app: &Value) -> Result<()> {
        self.call(
            reqwest::Method::PUT,
            &self.app_url(name),
            ARM,
            Some(app),
            &format!("containerApps create {name}"),
        )
        .await?
        .ok_or_else(|| {
            AlienError::new(ErrorData::CloudCallFailed {
                operation: format!("containerApps create {name}: the resource group is gone"),
            })
        })?;
        Ok(())
    }

    async fn patch(&self, name: &str, app: &Value) -> Result<()> {
        self.call(
            reqwest::Method::PATCH,
            &self.app_url(name),
            ARM,
            Some(app),
            &format!("containerApps patch {name}"),
        )
        .await?
        .ok_or_else(|| {
            AlienError::new(ErrorData::Refused {
                reason: format!("container app {name} does not exist; setup creates it"),
            })
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scale_set_url_keeps_the_name_in_one_path_segment() {
        let group = "/subscriptions/0000/resourceGroups/acme-prod";
        assert_eq!(
            scale_set_url(group, "acme-prod-compute-general").unwrap().as_str(),
            "https://management.azure.com/subscriptions/0000/resourceGroups/acme-prod/providers/Microsoft.Compute/virtualMachineScaleSets/acme-prod-compute-general?api-version=2024-07-01"
        );
        // A name the verifier never builds still can't climb out of the
        // scale set collection or add a query.
        assert_eq!(
            scale_set_url(group, "x/../../../other?api-version=1").unwrap().as_str(),
            "https://management.azure.com/subscriptions/0000/resourceGroups/acme-prod/providers/Microsoft.Compute/virtualMachineScaleSets/x%2F..%2F..%2F..%2Fother%3Fapi-version=1?api-version=2024-07-01"
        );
    }
}
