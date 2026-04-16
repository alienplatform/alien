/*!
# Azure Key Vault Client Integration Tests

Tests Azure Key Vault operations: vault management and secret operations.

## Prerequisites
Set up `.env.test` with Azure credentials:
```
AZURE_MANAGEMENT_SUBSCRIPTION_ID=your_subscription_id
AZURE_MANAGEMENT_TENANT_ID=your_tenant_id
AZURE_MANAGEMENT_CLIENT_ID=your_client_id
AZURE_MANAGEMENT_CLIENT_SECRET=your_client_secret
ALIEN_TEST_AZURE_RESOURCE_GROUP=your_test_resource_group
```

Note: The test creates vaults with access policies that grant your service principal permission to perform secret operations. The object ID will be automatically resolved by decoding the JWT token from Azure authentication if not provided explicitly.
*/

use alien_azure_clients::keyvault::{
    AzureKeyVaultCertificatesClient, AzureKeyVaultManagementClient, AzureKeyVaultSecretsClient,
    KeyVaultCertificatesApi, KeyVaultManagementApi, KeyVaultSecretsApi,
};
use alien_azure_clients::models::certificates::{CertificateBundle, CertificateImportParameters};
use alien_azure_clients::models::keyvault::{
    AccessPolicyEntry, Permissions, PermissionsSecretsItem, Sku, SkuFamily, SkuName,
    VaultCreateOrUpdateParameters, VaultProperties,
};
use alien_azure_clients::models::secrets::SecretSetParameters;
use alien_azure_clients::AzureTokenCache;
use alien_azure_clients::{AzureClientConfig, AzureCredentials};
use alien_client_core::{Error, ErrorData};
use alien_error::{AlienError, Context};
use base64::{engine::general_purpose, Engine as _};
use reqwest::Client;
use std::collections::{HashMap, HashSet};
use std::env;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};
use uuid::Uuid;

struct KeyVaultTestContext {
    management_client: AzureKeyVaultManagementClient,
    secrets_client: AzureKeyVaultSecretsClient,
    certificates_client: AzureKeyVaultCertificatesClient,
    subscription_id: String,
    resource_group_name: String,
    created_vaults: Mutex<HashSet<String>>,
    created_secrets: Mutex<HashMap<String, HashSet<String>>>, // vault_name -> set of secret names
    created_certificates: Mutex<HashMap<String, HashSet<String>>>, // vault_name -> set of certificate names
}

impl AsyncTestContext for KeyVaultTestContext {
    async fn setup() -> KeyVaultTestContext {
        let root: StdPathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
        tracing_subscriber::fmt::try_init().ok();

        let subscription_id = env::var("AZURE_MANAGEMENT_SUBSCRIPTION_ID")
            .expect("AZURE_MANAGEMENT_SUBSCRIPTION_ID not set");
        let tenant_id =
            env::var("AZURE_MANAGEMENT_TENANT_ID").expect("AZURE_MANAGEMENT_TENANT_ID not set");
        let client_id =
            env::var("AZURE_MANAGEMENT_CLIENT_ID").expect("AZURE_MANAGEMENT_CLIENT_ID not set");
        let client_secret = env::var("AZURE_MANAGEMENT_CLIENT_SECRET")
            .expect("AZURE_MANAGEMENT_CLIENT_SECRET not set");
        let resource_group_name = env::var("ALIEN_TEST_AZURE_RESOURCE_GROUP")
            .expect("ALIEN_TEST_AZURE_RESOURCE_GROUP not set");

        let config = AzureClientConfig {
            subscription_id: subscription_id.clone(),
            tenant_id,
            region: Some("eastus".to_string()),
            credentials: AzureCredentials::ServicePrincipal {
                client_id,
                client_secret,
            },
            service_overrides: None,
        };

        info!(
            "🔧 Using subscription: {} and resource group: {} for Key Vault testing",
            subscription_id, resource_group_name
        );

        let management_client =
            AzureKeyVaultManagementClient::new(Client::new(), AzureTokenCache::new(config.clone()));
        let secrets_client =
            AzureKeyVaultSecretsClient::new(Client::new(), AzureTokenCache::new(config.clone()));
        let certificates_client =
            AzureKeyVaultCertificatesClient::new(Client::new(), AzureTokenCache::new(config));

        KeyVaultTestContext {
            management_client,
            secrets_client,
            certificates_client,
            subscription_id,
            resource_group_name,
            created_vaults: Mutex::new(HashSet::new()),
            created_secrets: Mutex::new(HashMap::new()),
            created_certificates: Mutex::new(HashMap::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting Key Vault test cleanup...");

        // Cleanup certificates first
        let certificates_to_cleanup = {
            let certificates = self.created_certificates.lock().unwrap();
            certificates.clone()
        };

        for (vault_name, cert_names) in certificates_to_cleanup {
            for cert_name in cert_names {
                self.cleanup_certificate(&vault_name, &cert_name).await;
            }
        }

        // Cleanup secrets
        let secrets_to_cleanup = {
            let secrets = self.created_secrets.lock().unwrap();
            secrets.clone()
        };

        for (vault_name, secret_names) in secrets_to_cleanup {
            for secret_name in secret_names {
                self.cleanup_secret(&vault_name, &secret_name).await;
            }
        }

        // Cleanup vaults
        let vaults_to_cleanup = {
            let vaults = self.created_vaults.lock().unwrap();
            vaults.clone()
        };

        for vault_name in vaults_to_cleanup {
            self.cleanup_vault(&vault_name).await;
        }

        info!("✅ Key Vault test cleanup completed");
    }
}

impl KeyVaultTestContext {
    fn track_vault(&self, vault_name: &str) {
        let mut vaults = self.created_vaults.lock().unwrap();
        vaults.insert(vault_name.to_string());
        info!("📝 Tracking vault for cleanup: {}", vault_name);
    }

    fn untrack_vault(&self, vault_name: &str) {
        let mut vaults = self.created_vaults.lock().unwrap();
        vaults.remove(vault_name);
        info!(
            "✅ Vault {} successfully cleaned up and untracked",
            vault_name
        );
    }

    fn track_secret(&self, vault_name: &str, secret_name: &str) {
        let mut secrets = self.created_secrets.lock().unwrap();
        secrets
            .entry(vault_name.to_string())
            .or_insert_with(HashSet::new)
            .insert(secret_name.to_string());
        info!(
            "📝 Tracking secret for cleanup: {}/{}",
            vault_name, secret_name
        );
    }

    fn untrack_secret(&self, vault_name: &str, secret_name: &str) {
        let mut secrets = self.created_secrets.lock().unwrap();
        if let Some(vault_secrets) = secrets.get_mut(vault_name) {
            vault_secrets.remove(secret_name);
            if vault_secrets.is_empty() {
                secrets.remove(vault_name);
            }
        }
        info!(
            "✅ Secret {}/{} successfully cleaned up and untracked",
            vault_name, secret_name
        );
    }

    fn track_certificate(&self, vault_name: &str, cert_name: &str) {
        let mut certificates = self.created_certificates.lock().unwrap();
        certificates
            .entry(vault_name.to_string())
            .or_insert_with(HashSet::new)
            .insert(cert_name.to_string());
        info!(
            "📝 Tracking certificate for cleanup: {}/{}",
            vault_name, cert_name
        );
    }

    fn untrack_certificate(&self, vault_name: &str, cert_name: &str) {
        let mut certificates = self.created_certificates.lock().unwrap();
        if let Some(vault_certs) = certificates.get_mut(vault_name) {
            vault_certs.remove(cert_name);
            if vault_certs.is_empty() {
                certificates.remove(vault_name);
            }
        }
        info!(
            "✅ Certificate {}/{} successfully cleaned up and untracked",
            vault_name, cert_name
        );
    }

    async fn cleanup_vault(&self, vault_name: &str) {
        info!("🧹 Cleaning up vault: {}", vault_name);

        match self
            .management_client
            .delete_vault(self.resource_group_name.clone(), vault_name.to_string())
            .await
        {
            Ok(_) => {
                info!("✅ Vault {} deleted successfully", vault_name);
            }
            Err(e) => {
                if !matches!(
                    e,
                    Error {
                        error: Some(ErrorData::RemoteResourceNotFound { .. }),
                        ..
                    }
                ) {
                    warn!(
                        "Failed to delete vault {} during cleanup: {:?}",
                        vault_name, e
                    );
                }
            }
        }
    }

    async fn cleanup_secret(&self, vault_name: &str, secret_name: &str) {
        info!("🧹 Cleaning up secret: {}/{}", vault_name, secret_name);

        match self
            .secrets_client
            .delete_secret(
                format!("{}.vault.azure.net", vault_name),
                secret_name.to_string(),
            )
            .await
        {
            Ok(_) => {
                info!(
                    "✅ Secret {}/{} deleted successfully",
                    vault_name, secret_name
                );
            }
            Err(e) => {
                if !matches!(
                    e,
                    Error {
                        error: Some(ErrorData::RemoteResourceNotFound { .. }),
                        ..
                    }
                ) {
                    warn!(
                        "Failed to delete secret {}/{} during cleanup: {:?}",
                        vault_name, secret_name, e
                    );
                }
            }
        }
    }

    async fn cleanup_certificate(&self, vault_name: &str, cert_name: &str) {
        info!("🧹 Cleaning up certificate: {}/{}", vault_name, cert_name);

        match self
            .certificates_client
            .delete_certificate(
                format!("{}.vault.azure.net", vault_name),
                cert_name.to_string(),
            )
            .await
        {
            Ok(_) => {
                info!(
                    "✅ Certificate {}/{} deleted successfully",
                    vault_name, cert_name
                );
            }
            Err(e) => {
                if !matches!(
                    e,
                    Error {
                        error: Some(ErrorData::RemoteResourceNotFound { .. }),
                        ..
                    }
                ) {
                    warn!(
                        "Failed to delete certificate {}/{} during cleanup: {:?}",
                        vault_name, cert_name, e
                    );
                }
            }
        }
    }

    fn generate_unique_vault_name(&self) -> String {
        // Azure Key Vault names must be 3-24 alphanumeric characters, start with letter, end with letter/digit
        format!(
            "alientest{}",
            Uuid::new_v4().simple().to_string()[..8].to_lowercase()
        )
    }

    fn generate_unique_secret_name(&self) -> String {
        format!(
            "aliensecret{}",
            Uuid::new_v4().simple().to_string()[..6].to_lowercase()
        )
    }

    fn generate_unique_cert_name(&self) -> String {
        format!(
            "aliencert{}",
            Uuid::new_v4().simple().to_string()[..6].to_lowercase()
        )
    }

    async fn create_test_vault(&self, vault_name: &str) -> Result<(), Error> {
        let tenant_id =
            env::var("AZURE_MANAGEMENT_TENANT_ID").expect("AZURE_MANAGEMENT_TENANT_ID not set");
        let tenant_uuid = Uuid::parse_str(&tenant_id).expect("Invalid tenant ID format");

        // Get the service principal object ID for access policies
        let management_principal_id = self.resolve_service_principal_object_id().await?;

        // Create access policy for the service principal with secret and certificate permissions
        let access_policy = AccessPolicyEntry {
            object_id: management_principal_id,
            tenant_id: tenant_uuid,
            application_id: None,
            permissions: Permissions {
                secrets: vec![
                    PermissionsSecretsItem::Get,
                    PermissionsSecretsItem::Set,
                    PermissionsSecretsItem::List,
                    PermissionsSecretsItem::Delete,
                ],
                keys: vec![],
                certificates: vec![
                    alien_azure_clients::models::keyvault::PermissionsCertificatesItem::Get,
                    alien_azure_clients::models::keyvault::PermissionsCertificatesItem::Import,
                    alien_azure_clients::models::keyvault::PermissionsCertificatesItem::List,
                    alien_azure_clients::models::keyvault::PermissionsCertificatesItem::Delete,
                ],
                storage: vec![],
            },
        };

        let vault_properties = VaultProperties {
            tenant_id: tenant_uuid,
            sku: Sku {
                name: SkuName::Standard,
                family: SkuFamily::A,
            },
            access_policies: vec![access_policy],
            enable_rbac_authorization: false,
            enable_soft_delete: true,
            enabled_for_deployment: false,
            enabled_for_disk_encryption: false,
            enabled_for_template_deployment: false,
            private_endpoint_connections: vec![],
            public_network_access: "Enabled".to_string(),
            soft_delete_retention_in_days: 7,
            vault_uri: None,
            enable_purge_protection: None,
            network_acls: None,
            create_mode: None,
            provisioning_state: None,
            hsm_pool_resource_id: None,
        };

        let vault_params = VaultCreateOrUpdateParameters {
            location: "East US".to_string(),
            properties: vault_properties,
            tags: {
                let mut tags = HashMap::new();
                tags.insert("Environment".to_string(), "Test".to_string());
                tags.insert("Application".to_string(), "alien-test".to_string());
                tags
            },
        };

        let result = self
            .management_client
            .create_or_update_vault(
                self.resource_group_name.clone(),
                vault_name.to_string(),
                vault_params,
            )
            .await;
        if result.is_ok() {
            self.track_vault(vault_name);
        }
        result.map(|_| ())
    }

    async fn wait_for_vault_ready(&self, _vault_name: &str) {
        // Wait for vault to be ready for operations
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    }

    /// Automatically resolve the service principal's object ID by decoding the JWT token
    async fn resolve_service_principal_object_id(&self) -> Result<String, Error> {
        info!("🔍 Auto-resolving object ID from JWT token...");

        // Get a bearer token for Azure Resource Manager (this will contain the oid claim)
        let bearer_token = self
            .management_client
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await
            .context(ErrorData::HttpRequestFailed {
                message: "Failed to get bearer token".to_string(),
            })?;

        // Parse the JWT token to extract the payload (claims)
        let parts: Vec<&str> = bearer_token.split('.').collect();
        if parts.len() != 3 {
            return Err(AlienError::new(ErrorData::InvalidClientConfig {
                message: "Invalid JWT token format - expected 3 parts".to_string(),
                errors: None,
            }));
        }

        // Decode the payload (claims) part
        let claims_b64 = parts[1];
        let claims_bytes = general_purpose::URL_SAFE_NO_PAD
            .decode(claims_b64)
            .map_err(|e| {
                AlienError::new(ErrorData::DataLoadError {
                    message: format!("Failed to decode JWT payload: {}", e),
                })
            })?;

        // Parse the claims as JSON
        let claims_json: serde_json::Value =
            serde_json::from_slice(&claims_bytes).map_err(|e| {
                AlienError::new(ErrorData::DataLoadError {
                    message: format!("Failed to parse JWT claims JSON: {}", e),
                })
            })?;

        // Extract the oid (object ID) claim from the token
        let object_id = claims_json
            .get("oid")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AlienError::new(ErrorData::InvalidClientConfig {
                    message: "JWT token does not contain 'oid' claim (object ID)".to_string(),
                    errors: Some(format!("Available claims: {}", claims_json)),
                })
            })?;

        info!("✅ Auto-resolved object ID from JWT: {}", object_id);
        Ok(object_id.to_string())
    }

    async fn create_test_secret(
        &self,
        vault_name: &str,
        secret_name: &str,
        secret_value: &str,
    ) -> Result<(), Error> {
        let secret_params = SecretSetParameters {
            value: secret_value.to_string(),
            content_type: None,
            attributes: None,
            tags: HashMap::new(),
        };

        let result = self
            .secrets_client
            .set_secret(
                format!("{}.vault.azure.net", vault_name),
                secret_name.to_string(),
                secret_params,
            )
            .await;
        if result.is_ok() {
            self.track_secret(vault_name, secret_name);
        }
        result.map(|_| ())
    }
}

#[test_context(KeyVaultTestContext)]
#[tokio::test]
async fn test_vault_operations(ctx: &mut KeyVaultTestContext) {
    let vault_name = ctx.generate_unique_vault_name();
    info!("Testing vault operations: {}", vault_name);

    // Create vault
    ctx.create_test_vault(&vault_name)
        .await
        .expect("Failed to create vault");

    // Verify we can get the vault
    let vault = ctx
        .management_client
        .get_vault(ctx.resource_group_name.clone(), vault_name.clone())
        .await
        .expect("Failed to get created vault");
    assert_eq!(vault.name.as_ref().unwrap(), &vault_name);
}

#[test_context(KeyVaultTestContext)]
#[tokio::test]
async fn test_secret_operations(ctx: &mut KeyVaultTestContext) {
    let vault_name = ctx.generate_unique_vault_name();
    let secret_name = ctx.generate_unique_secret_name();
    let secret_value = "test-secret-value";

    info!("Testing secret operations: {}/{}", vault_name, secret_name);

    // Create vault and wait for it to be ready
    ctx.create_test_vault(&vault_name)
        .await
        .expect("Failed to create vault");
    ctx.wait_for_vault_ready(&vault_name).await;

    // Set secret
    ctx.create_test_secret(&vault_name, &secret_name, secret_value)
        .await
        .expect("Failed to set secret");

    // Get secret
    let secret = ctx
        .secrets_client
        .get_secret(
            format!("{}.vault.azure.net", vault_name),
            secret_name.clone(),
            None,
        )
        .await
        .expect("Failed to get secret");
    assert_eq!(secret.value.unwrap(), secret_value);

    // Update secret
    let updated_value = "updated-secret-value";
    let update_params = SecretSetParameters {
        value: updated_value.to_string(),
        content_type: None,
        attributes: None,
        tags: HashMap::new(),
    };

    let updated_secret = ctx
        .secrets_client
        .set_secret(
            format!("{}.vault.azure.net", vault_name),
            secret_name.clone(),
            update_params,
        )
        .await
        .expect("Failed to update secret");
    assert_eq!(updated_secret.value.unwrap(), updated_value);

    // Delete secret
    let deleted_secret = ctx
        .secrets_client
        .delete_secret(
            format!("{}.vault.azure.net", vault_name),
            secret_name.clone(),
        )
        .await
        .expect("Failed to delete secret");
    assert!(deleted_secret.id.is_some());
    ctx.untrack_secret(&vault_name, &secret_name); // Untrack since we manually deleted it
}

#[test_context(KeyVaultTestContext)]
#[tokio::test]
async fn test_certificate_import_and_delete(ctx: &mut KeyVaultTestContext) {
    let vault_name = ctx.generate_unique_vault_name();
    let cert_name = ctx.generate_unique_cert_name();

    info!(
        "Testing certificate import/delete operations: {}/{}",
        vault_name, cert_name
    );

    // Create vault and wait for it to be ready
    ctx.create_test_vault(&vault_name)
        .await
        .expect("Failed to create vault");
    ctx.wait_for_vault_ready(&vault_name).await;

    // Generate a valid self-signed certificate using rcgen
    use rcgen::{CertificateParams, DistinguishedName, DnType};

    let mut params = CertificateParams::new(vec!["example.com".to_string()])
        .expect("Failed to create certificate params");

    // Set distinguished name with Common Name
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "example.com");
    dn.push(DnType::OrganizationName, "Alien Test");
    dn.push(DnType::CountryName, "US");
    params.distinguished_name = dn;

    // Add Subject Alternative Names
    params.subject_alt_names = vec![rcgen::SanType::DnsName(
        rcgen::Ia5String::try_from("example.com").unwrap(),
    )];

    let key_pair = rcgen::KeyPair::generate().expect("Failed to generate key pair");
    let cert = params
        .self_signed(&key_pair)
        .expect("Failed to generate certificate");

    let certificate_pem = cert.pem();
    let private_key_pem = key_pair.serialize_pem();

    // Convert to PKCS#12 format using the same approach as Azure infra controller
    use p12::PFX;
    use pem::parse_many;

    // Parse private key PEM
    let key_blocks =
        parse_many(private_key_pem.as_bytes()).expect("Failed to parse private key PEM");
    let key_block = key_blocks
        .into_iter()
        .find(|p| p.tag().ends_with("PRIVATE KEY"))
        .expect("No PRIVATE KEY block found");
    let key_der = key_block.contents().to_vec();

    // Parse certificate PEM
    let cert_blocks =
        parse_many(certificate_pem.as_bytes()).expect("Failed to parse certificate PEM");
    let cert_block = cert_blocks
        .into_iter()
        .find(|p| p.tag().contains("CERTIFICATE"))
        .expect("No CERTIFICATE block found");
    let cert_der = cert_block.contents().to_vec();

    // Build PFX with empty password
    let pfx = PFX::new(&cert_der, &key_der, None, "", "Alien Test Certificate")
        .expect("Failed to create PKCS#12");
    let pfx_der = pfx.to_der();
    let pfx_base64 = base64::engine::general_purpose::STANDARD.encode(&pfx_der);

    let params = CertificateImportParameters {
        value: pfx_base64,
        pwd: Some("".to_string()), // Empty password
        policy: None,
        attributes: None,
        tags: Default::default(),
        preserve_cert_order: None,
    };

    let import_result = ctx
        .certificates_client
        .import_certificate(
            format!("{}.vault.azure.net", vault_name),
            cert_name.clone(),
            params,
        )
        .await
        .expect("Failed to import certificate");

    ctx.track_certificate(&vault_name, &cert_name);

    assert!(import_result.id.is_some(), "Certificate should have an ID");
    assert!(
        import_result.x5t.is_some(),
        "Certificate should have a thumbprint"
    );
    info!("✅ Certificate imported successfully");

    // Delete certificate
    let deleted_cert = ctx
        .certificates_client
        .delete_certificate(format!("{}.vault.azure.net", vault_name), cert_name.clone())
        .await
        .expect("Failed to delete certificate");
    assert!(deleted_cert.id.is_some());
    ctx.untrack_certificate(&vault_name, &cert_name);
    info!("✅ Certificate deleted successfully");
}

#[test_context(KeyVaultTestContext)]
#[tokio::test]
async fn test_error_handling(ctx: &mut KeyVaultTestContext) {
    // Test non-existent vault
    let non_existent_vault = "alien-test-non-existent-vault-12345";
    let result = ctx
        .management_client
        .get_vault(
            ctx.resource_group_name.clone(),
            non_existent_vault.to_string(),
        )
        .await;
    assert!(result.is_err());
    match result.unwrap_err() {
        Error {
            error: Some(ErrorData::RemoteResourceNotFound { resource_name, .. }),
            ..
        } => {
            assert_eq!(resource_name, non_existent_vault);
        }
        other => panic!("Expected RemoteResourceNotFound, got: {:?}", other),
    }

    // Test non-existent secret
    let vault_name = ctx.generate_unique_vault_name();
    ctx.create_test_vault(&vault_name)
        .await
        .expect("Failed to create vault for error test");
    ctx.wait_for_vault_ready(&vault_name).await;

    let non_existent_secret = "non-existent-secret-12345";
    let result = ctx
        .secrets_client
        .get_secret(
            format!("{}.vault.azure.net", vault_name),
            non_existent_secret.to_string(),
            None,
        )
        .await;
    assert!(result.is_err());
    match result.unwrap_err() {
        Error {
            error:
                Some(ErrorData::RemoteResourceNotFound {
                    resource_type,
                    resource_name,
                    ..
                }),
            ..
        } => {
            assert_eq!(resource_type, "Azure Key Vault Secret");
            assert_eq!(resource_name, non_existent_secret);
        }
        other => panic!("Expected RemoteResourceNotFound, got: {:?}", other),
    }
}

#[test_context(KeyVaultTestContext)]
#[tokio::test]
async fn test_invalid_credentials(_ctx: &mut KeyVaultTestContext) {
    let invalid_config = AzureClientConfig {
        subscription_id: "fake-subscription".to_string(),
        tenant_id: "fake-tenant".to_string(),
        region: Some("eastus".to_string()),
        credentials: AzureCredentials::ServicePrincipal {
            client_id: "fake-client-id".to_string(),
            client_secret: "fake-client-secret".to_string(),
        },
        service_overrides: None,
    };
    let invalid_client =
        AzureKeyVaultManagementClient::new(Client::new(), AzureTokenCache::new(invalid_config));

    let result = invalid_client
        .get_vault("fake-rg".to_string(), "fake-vault".to_string())
        .await;
    assert!(result.is_err());
}
