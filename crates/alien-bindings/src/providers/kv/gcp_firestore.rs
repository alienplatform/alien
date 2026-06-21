use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::time::Duration;

use alien_core::{GcpClientConfig, GcpCredentials, GcpImpersonationConfig};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use base64::{self, Engine};
use chrono::{DateTime, Utc};
use google_cloud_auth::credentials::{
    self, CacheableResource, Credentials, CredentialsProvider, EntityTag,
};
use google_cloud_auth::errors::CredentialsError;
use http::{header::AUTHORIZATION, Extensions, HeaderMap, HeaderValue};
use reqwest::{Client, Method, Response, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};

use crate::error::{ErrorData, Result};
use crate::traits::{Binding, Kv, PutOptions, ScanResult};

use super::{validate_key, validate_value};

const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
const FIRESTORE_REST_BASE_URL: &str = "https://firestore.googleapis.com/v1";

/// Firestore document for KV storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct KvDocument {
    value: String,
    created_at: DateTime<Utc>,
    expires_at: Option<DateTime<Utc>>,
}

/// GCP Firestore implementation of the KV trait.
pub struct GcpFirestoreKv {
    project_id: String,
    database_id: String,
    collection_name: String,
    endpoint: String,
    client: Client,
    credentials: Credentials,
}

impl Debug for GcpFirestoreKv {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GcpFirestoreKv")
            .field("project_id", &self.project_id)
            .field("database_id", &self.database_id)
            .field("collection_name", &self.collection_name)
            .field("endpoint", &self.endpoint)
            .finish()
    }
}

impl GcpFirestoreKv {
    pub fn new(
        gcp_config: GcpClientConfig,
        project_id: String,
        database_id: String,
        collection_name: String,
    ) -> Result<Self> {
        let endpoint = gcp_config
            .service_overrides
            .as_ref()
            .and_then(|overrides| overrides.endpoints.get("firestore"))
            .cloned()
            .unwrap_or_else(|| FIRESTORE_REST_BASE_URL.to_string());

        Ok(Self {
            project_id,
            database_id,
            collection_name,
            endpoint,
            client: crate::http_client::create_http_client(),
            credentials: credentials_from_gcp_config(&gcp_config)?,
        })
    }

    /// Checks if an item has expired based on TTL.
    fn is_expired(&self, expires_at: Option<DateTime<Utc>>) -> bool {
        expires_at.is_some_and(|expiry| Utc::now() >= expiry)
    }

    fn documents_root(&self) -> String {
        format!(
            "projects/{}/databases/{}/documents",
            self.project_id, self.database_id
        )
    }

    fn document_name(&self, key: &str) -> String {
        format!("{}/{}/{}", self.documents_root(), self.collection_name, key)
    }

    fn build_url(&self, path: &str, query_params: &[(&str, String)]) -> Result<Url> {
        let mut url = Url::parse(&format!(
            "{}/{}",
            self.endpoint.trim_end_matches('/'),
            path.trim_start_matches('/')
        ))
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "kv.firestore".to_string(),
            reason: format!("Invalid Firestore URL for path '{path}'"),
        })?;

        if !query_params.is_empty() {
            let mut query = url.query_pairs_mut();
            for (key, value) in query_params {
                query.append_pair(key, value);
            }
        }

        Ok(url)
    }

    async fn authed_request(&self, method: Method, url: Url) -> Result<reqwest::RequestBuilder> {
        let headers = match self
            .credentials
            .headers(Extensions::new())
            .await
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "kv.firestore".to_string(),
                reason: "Failed to get Google auth headers".to_string(),
            })? {
            CacheableResource::New { data, .. } => data,
            CacheableResource::NotModified => {
                return Err(AlienError::new(ErrorData::BindingSetupFailed {
                    binding_type: "kv.firestore".to_string(),
                    reason: "Google auth returned NotModified without cached headers".to_string(),
                }));
            }
        };

        Ok(self.client.request(method, url).headers(headers))
    }

    fn kv_document_to_firestore(
        &self,
        key: Option<&str>,
        kv_doc: &KvDocument,
    ) -> FirestoreDocument {
        let mut fields = HashMap::new();
        fields.insert(
            "value".to_string(),
            FirestoreValue::StringValue(kv_doc.value.clone()),
        );
        fields.insert(
            "created_at".to_string(),
            FirestoreValue::TimestampValue(kv_doc.created_at.to_rfc3339()),
        );

        if let Some(expires_at) = kv_doc.expires_at {
            fields.insert(
                "expires_at".to_string(),
                FirestoreValue::TimestampValue(expires_at.to_rfc3339()),
            );
        }

        FirestoreDocument {
            name: key.map(|key| self.document_name(key)),
            fields,
        }
    }

    fn firestore_to_kv_document(&self, doc: &FirestoreDocument) -> Result<KvDocument> {
        let value = match doc.fields.get("value") {
            Some(FirestoreValue::StringValue(value)) => value.clone(),
            _ => {
                return Err(AlienError::new(ErrorData::UnexpectedResponseFormat {
                    provider: "gcp".to_string(),
                    binding_name: "firestore".to_string(),
                    field: "value".to_string(),
                    response_json: serde_json::to_string(doc).unwrap_or_default(),
                }))
            }
        };

        let created_at = match doc.fields.get("created_at") {
            Some(FirestoreValue::TimestampValue(value)) => DateTime::parse_from_rfc3339(value)
                .map_err(|_| {
                    AlienError::new(ErrorData::UnexpectedResponseFormat {
                        provider: "gcp".to_string(),
                        binding_name: "firestore".to_string(),
                        field: "created_at".to_string(),
                        response_json: serde_json::to_string(doc).unwrap_or_default(),
                    })
                })?
                .with_timezone(&Utc),
            _ => {
                return Err(AlienError::new(ErrorData::UnexpectedResponseFormat {
                    provider: "gcp".to_string(),
                    binding_name: "firestore".to_string(),
                    field: "created_at".to_string(),
                    response_json: serde_json::to_string(doc).unwrap_or_default(),
                }))
            }
        };

        let expires_at = match doc.fields.get("expires_at") {
            Some(FirestoreValue::TimestampValue(value)) => Some(
                DateTime::parse_from_rfc3339(value)
                    .map_err(|_| {
                        AlienError::new(ErrorData::UnexpectedResponseFormat {
                            provider: "gcp".to_string(),
                            binding_name: "firestore".to_string(),
                            field: "expires_at".to_string(),
                            response_json: serde_json::to_string(doc).unwrap_or_default(),
                        })
                    })?
                    .with_timezone(&Utc),
            ),
            _ => None,
        };

        Ok(KvDocument {
            value,
            created_at,
            expires_at,
        })
    }

    async fn get_document(&self, key: &str) -> Result<Option<FirestoreDocument>> {
        let url = self.build_url(&self.document_name(key), &[])?;
        let response = self
            .authed_request(Method::GET, url.clone())
            .await?
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::KvOperationFailed {
                operation: "get".to_string(),
                key: key.to_string(),
                reason: "Failed to send Firestore get request".to_string(),
            })?;

        if response.status().as_u16() == 404 {
            return Ok(None);
        }

        ensure_success(response, "get", key, url)
            .await?
            .json::<FirestoreDocument>()
            .await
            .into_alien_error()
            .context(ErrorData::UnexpectedResponseFormat {
                provider: "gcp".to_string(),
                binding_name: "firestore".to_string(),
                field: "document".to_string(),
                response_json: String::new(),
            })
            .map(Some)
    }
}

impl Binding for GcpFirestoreKv {}

#[async_trait]
impl Kv for GcpFirestoreKv {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        validate_key(key)?;

        let Some(doc) = self.get_document(key).await? else {
            return Ok(None);
        };
        let kv_doc = self.firestore_to_kv_document(&doc)?;

        if self.is_expired(kv_doc.expires_at) {
            return Ok(None);
        }

        base64::engine::general_purpose::STANDARD
            .decode(&kv_doc.value)
            .into_alien_error()
            .context(ErrorData::KvOperationFailed {
                operation: "get".to_string(),
                key: key.to_string(),
                reason: "Failed to decode base64 value".to_string(),
            })
            .map(Some)
    }

    async fn put(&self, key: &str, value: Vec<u8>, options: Option<PutOptions>) -> Result<bool> {
        validate_key(key)?;
        validate_value(&value)?;

        let options = options.unwrap_or_default();
        let encoded_value = base64::engine::general_purpose::STANDARD.encode(&value);
        let kv_doc = KvDocument {
            value: encoded_value,
            created_at: Utc::now(),
            expires_at: options.ttl.map(|duration| Utc::now() + duration),
        };

        if options.if_not_exists {
            let document = self.kv_document_to_firestore(None, &kv_doc);
            let url = self.build_url(
                &format!("{}/{}", self.documents_root(), self.collection_name),
                &[("documentId", key.to_string())],
            )?;
            let response = self
                .authed_request(Method::POST, url.clone())
                .await?
                .json(&document)
                .send()
                .await
                .into_alien_error()
                .context(ErrorData::KvOperationFailed {
                    operation: "put".to_string(),
                    key: key.to_string(),
                    reason: "Failed to send Firestore create request".to_string(),
                })?;

            if response.status().as_u16() == 409 {
                return Ok(false);
            }

            ensure_success(response, "create", key, url).await?;
            Ok(true)
        } else {
            let document = self.kv_document_to_firestore(Some(key), &kv_doc);
            let url = self.build_url(&self.document_name(key), &[])?;
            let response = self
                .authed_request(Method::PATCH, url.clone())
                .await?
                .json(&document)
                .send()
                .await
                .into_alien_error()
                .context(ErrorData::KvOperationFailed {
                    operation: "put".to_string(),
                    key: key.to_string(),
                    reason: "Failed to send Firestore patch request".to_string(),
                })?;

            ensure_success(response, "patch", key, url).await?;
            Ok(true)
        }
    }

    async fn delete(&self, key: &str) -> Result<()> {
        validate_key(key)?;

        let url = self.build_url(&self.document_name(key), &[])?;
        let response = self
            .authed_request(Method::DELETE, url.clone())
            .await?
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::KvOperationFailed {
                operation: "delete".to_string(),
                key: key.to_string(),
                reason: "Failed to send Firestore delete request".to_string(),
            })?;

        ensure_success(response, "delete", key, url).await?;
        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        validate_key(key)?;

        let Some(doc) = self.get_document(key).await? else {
            return Ok(false);
        };
        let kv_doc = self.firestore_to_kv_document(&doc)?;

        Ok(!self.is_expired(kv_doc.expires_at))
    }

    async fn scan_prefix(
        &self,
        prefix: &str,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> Result<ScanResult> {
        validate_key(prefix)?;

        let mut structured_query = StructuredQuery {
            from: vec![CollectionSelector {
                collection_id: self.collection_name.clone(),
            }],
            order_by: vec![Order {
                field: FieldReference {
                    field_path: "__name__".to_string(),
                },
                direction: "ASCENDING".to_string(),
            }],
            r#where: None,
            limit: limit.map(|limit| limit as i32),
            offset: cursor
                .as_deref()
                .and_then(|cursor| cursor.parse::<i32>().ok()),
        };

        if !prefix.is_empty() {
            structured_query.r#where = Some(Filter::FieldFilter(FieldFilter {
                field: FieldReference {
                    field_path: "__name__".to_string(),
                },
                op: "GREATER_THAN_OR_EQUAL".to_string(),
                value: FirestoreValue::ReferenceValue(self.document_name(prefix)),
            }));
        }

        let query_request = RunQueryRequest { structured_query };
        let url = self.build_url(&format!("{}:runQuery", self.documents_root()), &[])?;
        let response = self
            .authed_request(Method::POST, url.clone())
            .await?
            .json(&query_request)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::KvOperationFailed {
                operation: "scan_prefix".to_string(),
                key: prefix.to_string(),
                reason: "Failed to send Firestore runQuery request".to_string(),
            })?;

        let query_responses = ensure_success(response, "runQuery", prefix, url)
            .await?
            .json::<Vec<RunQueryResponse>>()
            .await
            .into_alien_error()
            .context(ErrorData::UnexpectedResponseFormat {
                provider: "gcp".to_string(),
                binding_name: "firestore".to_string(),
                field: "runQuery".to_string(),
                response_json: String::new(),
            })?;

        let items: Vec<(String, Vec<u8>)> = query_responses
            .iter()
            .filter_map(|response| {
                let doc = response.document.as_ref()?;
                let doc_name = doc.name.as_ref()?;
                let key = doc_name.split('/').last()?.to_string();

                if !key.starts_with(prefix) {
                    return None;
                }

                let kv_doc = self.firestore_to_kv_document(doc).ok()?;
                if self.is_expired(kv_doc.expires_at) {
                    return None;
                }

                let value = base64::engine::general_purpose::STANDARD
                    .decode(&kv_doc.value)
                    .ok()?;
                Some((key, value))
            })
            .collect();

        let next_cursor = if items.len() == limit.unwrap_or(usize::MAX) {
            let current_offset = cursor
                .as_ref()
                .and_then(|cursor| cursor.parse::<usize>().ok())
                .unwrap_or(0);
            Some((current_offset + items.len()).to_string())
        } else {
            None
        };

        Ok(ScanResult { items, next_cursor })
    }
}

async fn ensure_success(
    response: Response,
    operation: &str,
    key: &str,
    url: Url,
) -> Result<Response> {
    if response.status().is_success() {
        return Ok(response);
    }

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    Err(AlienError::new(ErrorData::KvOperationFailed {
        operation: operation.to_string(),
        key: key.to_string(),
        reason: format!("Firestore request to {url} failed with status {status}: {body}"),
    }))
}

#[derive(Debug, Clone)]
struct StaticAccessTokenCredentials {
    token: String,
    entity_tag: EntityTag,
}

impl StaticAccessTokenCredentials {
    fn new(token: String) -> Self {
        Self {
            token,
            entity_tag: EntityTag::new(),
        }
    }
}

impl CredentialsProvider for StaticAccessTokenCredentials {
    fn headers(
        &self,
        _extensions: Extensions,
    ) -> impl Future<Output = std::result::Result<CacheableResource<HeaderMap>, CredentialsError>> + Send
    {
        let token = self.token.clone();
        let entity_tag = self.entity_tag.clone();
        async move {
            let mut value = HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|error| CredentialsError::from_source(false, error))?;
            value.set_sensitive(true);

            let mut headers = HeaderMap::new();
            headers.insert(AUTHORIZATION, value);

            Ok(CacheableResource::New {
                entity_tag,
                data: headers,
            })
        }
    }

    fn universe_domain(&self) -> impl Future<Output = Option<String>> + Send {
        async { None }
    }
}

fn credentials_from_gcp_config(config: &GcpClientConfig) -> Result<Credentials> {
    credentials_from_gcp_credentials(&config.credentials)
}

fn credentials_from_gcp_credentials(credentials: &GcpCredentials) -> Result<Credentials> {
    match credentials {
        GcpCredentials::AccessToken { token } => {
            Ok(Credentials::from(StaticAccessTokenCredentials::new(token.clone())))
        }
        GcpCredentials::ServiceAccountKey { json } => {
            let key = serde_json::from_str::<JsonValue>(json).into_alien_error().context(
                ErrorData::BindingSetupFailed {
                    binding_type: "kv.firestore".to_string(),
                    reason: "Failed to parse GCP service account key JSON".to_string(),
                },
            )?;
            credentials::service_account::Builder::new(key)
                .with_access_specifier(credentials::service_account::AccessSpecifier::from_scopes(
                    [CLOUD_PLATFORM_SCOPE],
                ))
                .build()
                .into_alien_error()
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "kv.firestore".to_string(),
                    reason: "Failed to build official GCP service account credentials".to_string(),
                })
        }
        GcpCredentials::ServiceMetadata => credentials::mds::Builder::default()
            .with_scopes([CLOUD_PLATFORM_SCOPE])
            .build()
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "kv.firestore".to_string(),
                reason: "Failed to build official GCP metadata credentials".to_string(),
            }),
        GcpCredentials::ExternalAccount {
            audience,
            subject_token_type,
            token_url,
            credential_source_file,
            service_account_impersonation_url,
        } => {
            let external_account = external_account_json(
                audience,
                subject_token_type,
                token_url,
                credential_source_file,
                service_account_impersonation_url.as_deref(),
            );
            credentials::external_account::Builder::new(external_account)
                .build()
                .into_alien_error()
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "kv.firestore".to_string(),
                    reason: "Failed to build official GCP external account credentials".to_string(),
                })
        }
        GcpCredentials::AuthorizedUser {
            client_id,
            client_secret,
            refresh_token,
        } => {
            let authorized_user = json!({
                "type": "authorized_user",
                "client_id": client_id,
                "client_secret": client_secret,
                "refresh_token": refresh_token,
            });
            credentials::user_account::Builder::new(authorized_user)
                .with_scopes([CLOUD_PLATFORM_SCOPE])
                .build()
                .into_alien_error()
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "kv.firestore".to_string(),
                    reason: "Failed to build official GCP authorized user credentials".to_string(),
                })
        }
        GcpCredentials::ImpersonatedServiceAccount { source, config } => {
            impersonated_credentials_from_gcp_config(source, config)
        }
        GcpCredentials::ProjectedServiceAccount { .. } => Err(AlienError::new(
            ErrorData::BindingSetupFailed {
                binding_type: "kv.firestore".to_string(),
                reason: "Projected service account token files are not a complete official Google auth credential configuration; use external_account credentials with an audience and credential source instead".to_string(),
            },
        )),
    }
}

fn impersonated_credentials_from_gcp_config(
    source: &GcpClientConfig,
    config: &GcpImpersonationConfig,
) -> Result<Credentials> {
    let source_credentials = credentials_from_gcp_config(source)?;
    let mut builder =
        credentials::impersonated::Builder::from_source_credentials(source_credentials)
            .with_target_principal(config.service_account_email.clone())
            .with_scopes(config.scopes.clone());

    if let Some(delegates) = &config.delegates {
        builder = builder.with_delegates(delegates.clone());
    }

    if let Some(lifetime) = &config.lifetime {
        builder = builder.with_lifetime(parse_google_duration(lifetime)?);
    }

    builder
        .build()
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "kv.firestore".to_string(),
            reason: "Failed to build official GCP impersonated credentials".to_string(),
        })
}

fn external_account_json(
    audience: &str,
    subject_token_type: &str,
    token_url: &str,
    credential_source_file: &str,
    service_account_impersonation_url: Option<&str>,
) -> JsonValue {
    let mut value = json!({
        "type": "external_account",
        "audience": audience,
        "subject_token_type": subject_token_type,
        "token_url": token_url,
        "credential_source": {
            "file": credential_source_file,
        },
        "scopes": [CLOUD_PLATFORM_SCOPE],
    });

    if let Some(url) = service_account_impersonation_url {
        value["service_account_impersonation_url"] = JsonValue::String(url.to_string());
    }

    value
}

fn parse_google_duration(value: &str) -> Result<Duration> {
    let seconds = value
        .strip_suffix('s')
        .ok_or_else(|| {
            AlienError::new(ErrorData::BindingSetupFailed {
                binding_type: "kv.firestore".to_string(),
                reason: format!("Invalid Google duration '{}': missing 's' suffix", value),
            })
        })?
        .parse::<u64>()
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "kv.firestore".to_string(),
            reason: format!("Invalid Google duration '{}'", value),
        })?;

    Ok(Duration::from_secs(seconds))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FirestoreDocument {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default)]
    fields: HashMap<String, FirestoreValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum FirestoreValue {
    StringValue(String),
    TimestampValue(String),
    ReferenceValue(String),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunQueryRequest {
    structured_query: StructuredQuery,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StructuredQuery {
    from: Vec<CollectionSelector>,
    order_by: Vec<Order>,
    #[serde(rename = "where", skip_serializing_if = "Option::is_none")]
    r#where: Option<Filter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    offset: Option<i32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CollectionSelector {
    collection_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Order {
    field: FieldReference,
    direction: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FieldReference {
    field_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
enum Filter {
    FieldFilter(FieldFilter),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FieldFilter {
    field: FieldReference,
    op: String,
    value: FirestoreValue,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunQueryResponse {
    document: Option<FirestoreDocument>,
}
