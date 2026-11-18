//! User-delegation SAS for one directory of a hierarchical-namespace storage account.
//!
//! A directory SAS (`sr=d`) is the only Azure grant that confines a bearer to a path prefix:
//! a flat account can scope a SAS to a container or a single blob, nothing in between. It is
//! signed with a user delegation key the caller's Entra identity obtains, so it never needs the
//! account key and lives no longer than that key.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::{DateTime, SecondsFormat, Utc};
use hmac::{Hmac, Mac};
use reqwest::{Client, StatusCode};
use sha2::Sha256;

use crate::azure::common::create_azure_http_error_with_context;
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};

#[cfg(feature = "test-utils")]
use mockall::automock;

/// Storage service version the delegation key request and the SAS are signed for. The SAS
/// string-to-sign layout below is the one this version defines.
pub const SAS_VERSION: &str = "2026-10-06";

/// A user delegation key, as `Get User Delegation Key` returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserDelegationKey {
    /// Object id of the identity that requested the key.
    pub signed_oid: String,
    /// Tenant of that identity.
    pub signed_tid: String,
    /// When the key becomes valid (as Azure wrote it, ISO 8601).
    pub signed_start: String,
    /// When the key expires (as Azure wrote it, ISO 8601).
    pub signed_expiry: String,
    /// The service the key is for (`b`).
    pub signed_service: String,
    /// The service version that issued the key.
    pub signed_version: String,
    /// The base64 HMAC key.
    pub value: String,
}

/// Rights a directory SAS can carry. Only what persistent storage uses: no delete, list,
/// move or ACL rights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectorySasPermissions {
    /// Read blobs (`r`).
    pub read: bool,
    /// Add blocks (`a`).
    pub add: bool,
    /// Create new blobs (`c`).
    pub create: bool,
    /// Write and overwrite blobs (`w`).
    pub write: bool,
}

impl DirectorySasPermissions {
    /// `sp` in the canonical order Azure requires.
    fn as_sp(&self) -> String {
        [
            (self.read, 'r'),
            (self.add, 'a'),
            (self.create, 'c'),
            (self.write, 'w'),
        ]
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, c)| *c)
        .collect()
    }
}

/// Obtains user delegation keys from a storage account's blob service.
#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait BlobDelegationApi: Send + Sync + std::fmt::Debug {
    /// A delegation key valid from `start` to `expiry` (at most seven days apart).
    async fn get_user_delegation_key(
        &self,
        account: &str,
        start: DateTime<Utc>,
        expiry: DateTime<Utc>,
    ) -> Result<UserDelegationKey>;
}

/// The blob service client for delegation keys, authenticated with Entra.
#[derive(Debug)]
pub struct AzureBlobDelegationClient {
    client: Client,
    token_cache: AzureTokenCache,
}

impl AzureBlobDelegationClient {
    /// A client that authenticates with the given token cache.
    pub fn new(client: Client, token_cache: AzureTokenCache) -> Self {
        Self {
            client,
            token_cache,
        }
    }

    fn blob_endpoint(&self, account: &str) -> String {
        match self.token_cache.get_service_endpoint("blob") {
            Some(endpoint) => endpoint.trim_end_matches('/').to_string(),
            None => format!("https://{account}.blob.core.windows.net"),
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl BlobDelegationApi for AzureBlobDelegationClient {
    async fn get_user_delegation_key(
        &self,
        account: &str,
        start: DateTime<Utc>,
        expiry: DateTime<Utc>,
    ) -> Result<UserDelegationKey> {
        let url = format!(
            "{}/?restype=service&comp=userdelegationkey",
            self.blob_endpoint(account)
        );
        let body = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?><KeyInfo><Start>{}</Start><Expiry>{}</Expiry></KeyInfo>",
            sas_time(start),
            sas_time(expiry)
        );
        let token = self
            .token_cache
            .get_bearer_token_with_scope("https://storage.azure.com/.default")
            .await?;
        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("x-ms-version", SAS_VERSION)
            .header("Content-Type", "application/xml")
            .body(body.clone())
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: "Azure GetUserDelegationKey: failed to execute request".to_string(),
            })?;
        let status = response.status();
        let text =
            response
                .text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: "Azure GetUserDelegationKey: failed to read the response".to_string(),
                })?;
        if status != StatusCode::OK {
            return Err(create_azure_http_error_with_context(
                status,
                "GetUserDelegationKey",
                "Azure Storage Account",
                account,
                &text,
                &url,
                Some(body),
            ));
        }
        parse_user_delegation_key(&text)
    }
}

/// Reads a `UserDelegationKey` response body.
pub fn parse_user_delegation_key(xml: &str) -> Result<UserDelegationKey> {
    let field = |tag: &str| -> Result<String> {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        xml.split_once(&open)
            .and_then(|(_, rest)| rest.split_once(&close))
            .map(|(value, _)| value.to_string())
            .ok_or_else(|| {
                AlienError::new(ErrorData::DataLoadError {
                    message: format!("the user delegation key response has no {tag}"),
                })
            })
    };
    Ok(UserDelegationKey {
        signed_oid: field("SignedOid")?,
        signed_tid: field("SignedTid")?,
        signed_start: field("SignedStart")?,
        signed_expiry: field("SignedExpiry")?,
        signed_service: field("SignedService")?,
        signed_version: field("SignedVersion")?,
        value: field("Value")?,
    })
}

/// A SAS for `directory` (a path inside `container`, no leading or trailing `/`) and
/// everything below it, valid from `start` to `expiry`, HTTPS only. Returns the query string
/// without a leading `?`.
///
/// `expiry` must not be later than the delegation key's; Azure refuses such a SAS anyway, but
/// refusing it here names the cause.
pub fn directory_sas(
    key: &UserDelegationKey,
    account: &str,
    container: &str,
    directory: &str,
    permissions: DirectorySasPermissions,
    start: DateTime<Utc>,
    expiry: DateTime<Utc>,
) -> Result<String> {
    let invalid = |message: String| {
        AlienError::new(ErrorData::InvalidInput {
            message,
            field_name: Some("directory".to_string()),
        })
    };
    let components: Vec<&str> = directory.split('/').collect();
    if directory.is_empty()
        || components
            .iter()
            .any(|part| part.is_empty() || *part == "." || *part == "..")
    {
        return Err(invalid(format!(
            "'{directory}' is not a directory path: it must be relative, with no '.', '..' or empty component"
        )));
    }
    if !directory
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/'))
    {
        return Err(invalid(format!(
            "'{directory}' may contain only letters, digits, '.', '_', '-' and '/'"
        )));
    }
    if expiry <= start {
        return Err(invalid("the SAS expires before it starts".to_string()));
    }
    let key_expiry = DateTime::parse_from_rfc3339(&key.signed_expiry)
        .into_alien_error()
        .context(ErrorData::DataLoadError {
            message: "the user delegation key's expiry is not a timestamp".to_string(),
        })?;
    if expiry > key_expiry {
        return Err(invalid(
            "the SAS would outlive the user delegation key that signs it".to_string(),
        ));
    }
    let sp = permissions.as_sp();
    let st = sas_time(start);
    let se = sas_time(expiry);
    let resource = format!("/blob/{account}/{container}/{directory}");
    // The string-to-sign of a user delegation SAS for version 2025-07-05 and later. Empty
    // fields are the optional ones this SAS does not use, in Azure's order.
    let string_to_sign = [
        sp.as_str(),
        &st,
        &se,
        &resource,
        &key.signed_oid,
        &key.signed_tid,
        &key.signed_start,
        &key.signed_expiry,
        &key.signed_service,
        &key.signed_version,
        "", // signedAuthorizedUserObjectId
        "", // signedUnauthorizedUserObjectId
        "", // signedCorrelationId
        "", // signedKeyDelegatedUserTenantId
        "", // signedDelegatedUserObjectId
        "", // signedIP
        "https",
        SAS_VERSION,
        "d",
        "", // signedSnapshotTime
        "", // signedEncryptionScope
        "", // signedRequestHeaders
        "", // signedRequestQueryParameters
        "", // rscc
        "", // rscd
        "", // rsce
        "", // rscl
        "", // rsct
    ]
    .join("\n");
    let secret =
        STANDARD
            .decode(&key.value)
            .into_alien_error()
            .context(ErrorData::DataLoadError {
                message: "the user delegation key is not base64".to_string(),
            })?;
    let mut mac = Hmac::<Sha256>::new_from_slice(&secret).expect("HMAC takes a key of any length");
    mac.update(string_to_sign.as_bytes());
    let signature = STANDARD.encode(mac.finalize().into_bytes());

    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query
        .append_pair("st", &st)
        .append_pair("se", &se)
        .append_pair("sp", &sp)
        .append_pair("spr", "https")
        .append_pair("sv", SAS_VERSION)
        .append_pair("sr", "d")
        .append_pair("sdd", &components.len().to_string())
        .append_pair("skoid", &key.signed_oid)
        .append_pair("sktid", &key.signed_tid)
        .append_pair("skt", &key.signed_start)
        .append_pair("ske", &key.signed_expiry)
        .append_pair("sks", &key.signed_service)
        .append_pair("skv", &key.signed_version)
        .append_pair("sig", &signature);
    Ok(query.finish())
}

fn sas_time(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn key() -> UserDelegationKey {
        UserDelegationKey {
            signed_oid: "11111111-2222-3333-4444-555555555555".to_string(),
            signed_tid: "66666666-7777-8888-9999-000000000000".to_string(),
            signed_start: "2026-11-18T10:00:00Z".to_string(),
            signed_expiry: "2026-11-18T12:00:00Z".to_string(),
            signed_service: "b".to_string(),
            signed_version: "2023-11-03".to_string(),
            value: "c2VjcmV0LWRlbGVnYXRpb24ta2V5LWZvci10ZXN0cy0wMDAwMDAwMDAwMA==".to_string(),
        }
    }

    const RACW: DirectorySasPermissions = DirectorySasPermissions {
        read: true,
        add: true,
        create: true,
        write: true,
    };

    fn at(hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 18, hour, minute, 0).unwrap()
    }

    fn param(query: &str, name: &str) -> String {
        url::form_urlencoded::parse(query.as_bytes())
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.into_owned())
            .unwrap_or_default()
    }

    /// The same inputs given to Azure's Python SDK (`azure-storage-file-datalake` 12.26.1,
    /// `generate_directory_sas` with this key) produce this signature, so the string-to-sign
    /// matches Azure's own.
    #[test]
    fn signs_exactly_as_azures_sdk() {
        let sas = directory_sas(
            &key(),
            "acct1",
            "store",
            "sandbox-storage/agent/engine-42",
            RACW,
            at(10, 55),
            at(11, 55),
        )
        .unwrap();
        assert_eq!(
            param(&sas, "sig"),
            "bkdncRTVvx/hNuaFo6stzGmF1j6jlyla2N4yJvjjJ/c="
        );
        assert_eq!(param(&sas, "sr"), "d");
        assert_eq!(param(&sas, "sdd"), "3");
        assert_eq!(param(&sas, "sp"), "racw");
        assert_eq!(param(&sas, "spr"), "https");
        assert_eq!(param(&sas, "sv"), SAS_VERSION);
    }

    #[test]
    fn grants_only_what_was_asked() {
        let read_only = DirectorySasPermissions {
            read: true,
            add: false,
            create: false,
            write: false,
        };
        let sas = directory_sas(&key(), "a", "c", "d", read_only, at(10, 55), at(11, 0)).unwrap();
        assert_eq!(param(&sas, "sp"), "r");
        assert_eq!(param(&sas, "sdd"), "1");
    }

    #[test]
    fn refuses_a_directory_that_could_name_another() {
        for directory in [
            "", "/a", "a/", "a//b", "a/../b", "..", ".", "a/./b", "a b", "a%2fb", "a?b",
        ] {
            assert!(
                directory_sas(&key(), "a", "c", directory, RACW, at(10, 55), at(11, 0)).is_err(),
                "{directory:?}"
            );
        }
    }

    #[test]
    fn refuses_a_sas_that_outlives_its_key_or_ends_before_it_starts() {
        assert!(directory_sas(&key(), "a", "c", "d", RACW, at(10, 55), at(12, 1)).is_err());
        assert!(directory_sas(&key(), "a", "c", "d", RACW, at(11, 0), at(11, 0)).is_err());
    }

    #[test]
    fn parses_a_delegation_key_response() {
        let xml = "<?xml version=\"1.0\" encoding=\"utf-8\"?><UserDelegationKey>\
            <SignedOid>11111111-2222-3333-4444-555555555555</SignedOid>\
            <SignedTid>66666666-7777-8888-9999-000000000000</SignedTid>\
            <SignedStart>2026-11-18T10:00:00Z</SignedStart>\
            <SignedExpiry>2026-11-18T12:00:00Z</SignedExpiry>\
            <SignedService>b</SignedService><SignedVersion>2023-11-03</SignedVersion>\
            <Value>c2VjcmV0LWRlbGVnYXRpb24ta2V5LWZvci10ZXN0cy0wMDAwMDAwMDAwMA==</Value>\
            </UserDelegationKey>";
        assert_eq!(parse_user_delegation_key(xml).unwrap(), key());
        assert!(parse_user_delegation_key("<UserDelegationKey/>").is_err());
    }
}
