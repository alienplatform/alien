//! Integration tests for LocalVaultManager + LocalVault binding
//!
//! These tests verify that:
//! 1. Manager creates vault directories usable by LocalVault binding
//! 2. Health checks verify vault accessibility
//! 3. Secrets persist across sessions

use alien_bindings::providers::vault::local::LocalVault;
use alien_bindings::traits::Vault;
use alien_core::bindings::VaultBinding;
use alien_local::LocalVaultManager;
use tempfile::TempDir;

/// Manager creates vault that LocalVault binding can actually use
#[tokio::test]
async fn test_manager_creates_usable_vault() {
    let temp_dir = TempDir::new().unwrap();
    let manager = LocalVaultManager::new(temp_dir.path().to_path_buf());

    // Manager creates vault directory
    manager.create_vault("my-vault").await.unwrap();

    // Get path and create LocalVault
    let vault_path = manager.get_vault_path("my-vault").unwrap();
    let vault = LocalVault::new("my-vault".to_string(), vault_path);

    // Actually use it
    vault.set_secret("api-key", "secret-123").await.unwrap();
    let secret = vault.get_secret("api-key").await.unwrap();
    assert_eq!(secret, "secret-123");
}

/// Health check validates vault accessibility
#[tokio::test]
async fn test_health_check_validates_vault() {
    let temp_dir = TempDir::new().unwrap();
    let manager = LocalVaultManager::new(temp_dir.path().to_path_buf());

    // Missing vault fails health check
    assert!(manager.check_health("nonexistent").await.is_err());

    // Create and verify health passes
    manager.create_vault("healthy-vault").await.unwrap();
    manager.check_health("healthy-vault").await.unwrap();

    // Delete and verify health fails
    manager.delete_vault("healthy-vault").await.unwrap();
    assert!(manager.check_health("healthy-vault").await.is_err());
}

/// Vault secrets persist across sessions (simulates CLI restart)
#[tokio::test]
async fn test_vault_secrets_persist_across_sessions() {
    let temp_dir = TempDir::new().unwrap();

    // Write secret with first instance
    {
        let manager = LocalVaultManager::new(temp_dir.path().to_path_buf());
        manager.create_vault("persist-vault").await.unwrap();
        let vault_path = manager.get_vault_path("persist-vault").unwrap();
        let vault = LocalVault::new("persist-vault".to_string(), vault_path);
        vault.set_secret("db-password", "s3cr3t").await.unwrap();
    }

    // Read with fresh instance
    {
        let manager = LocalVaultManager::new(temp_dir.path().to_path_buf());
        // Vault should still exist
        assert!(manager.vault_exists("persist-vault"));

        let vault_path = manager.get_vault_path("persist-vault").unwrap();
        let vault = LocalVault::new("persist-vault".to_string(), vault_path);
        let secret = vault.get_secret("db-password").await.unwrap();
        assert_eq!(secret, "s3cr3t");
    }
}

/// get_binding returns correct VaultBinding::Local variant
#[tokio::test]
async fn test_get_binding_returns_local_variant() {
    let temp_dir = TempDir::new().unwrap();
    let manager = LocalVaultManager::new(temp_dir.path().to_path_buf());

    manager.create_vault("binding-test").await.unwrap();
    let binding = manager.get_binding("binding-test").unwrap();

    // Verify it's the Local variant
    match binding {
        VaultBinding::Local(config) => {
            let data_dir = config
                .data_dir
                .into_value("binding-test", "data_dir")
                .unwrap();
            assert!(data_dir.contains("binding-test"));
        }
        _ => panic!("Expected Local binding variant"),
    }
}

/// get_binding fails for non-existent vault
#[tokio::test]
async fn test_get_binding_fails_for_missing_vault() {
    let temp_dir = TempDir::new().unwrap();
    let manager = LocalVaultManager::new(temp_dir.path().to_path_buf());

    let result = manager.get_binding("nonexistent");
    assert!(result.is_err());
}

/// Multiple vaults can coexist
#[tokio::test]
async fn test_multiple_vaults_coexist() {
    let temp_dir = TempDir::new().unwrap();
    let manager = LocalVaultManager::new(temp_dir.path().to_path_buf());

    // Create multiple vaults
    manager.create_vault("vault-a").await.unwrap();
    manager.create_vault("vault-b").await.unwrap();

    // Get paths for both
    let path_a = manager.get_vault_path("vault-a").unwrap();
    let path_b = manager.get_vault_path("vault-b").unwrap();

    let vault_a = LocalVault::new("vault-a".to_string(), path_a);
    let vault_b = LocalVault::new("vault-b".to_string(), path_b);

    // Write same secret name to both
    vault_a.set_secret("key", "value-a").await.unwrap();
    vault_b.set_secret("key", "value-b").await.unwrap();

    // Verify data is isolated
    assert_eq!(vault_a.get_secret("key").await.unwrap(), "value-a");
    assert_eq!(vault_b.get_secret("key").await.unwrap(), "value-b");
}

/// Vault delete_secret functionality works
#[tokio::test]
async fn test_vault_delete_secret() {
    let temp_dir = TempDir::new().unwrap();
    let manager = LocalVaultManager::new(temp_dir.path().to_path_buf());

    manager.create_vault("delete-test").await.unwrap();
    let vault_path = manager.get_vault_path("delete-test").unwrap();
    let vault = LocalVault::new("delete-test".to_string(), vault_path);

    // Set and verify
    vault.set_secret("to-delete", "value").await.unwrap();
    assert_eq!(vault.get_secret("to-delete").await.unwrap(), "value");

    // Delete
    vault.delete_secret("to-delete").await.unwrap();

    // Should be gone
    assert!(vault.get_secret("to-delete").await.is_err());
}

/// Vault update secret works
#[tokio::test]
async fn test_vault_update_secret() {
    let temp_dir = TempDir::new().unwrap();
    let manager = LocalVaultManager::new(temp_dir.path().to_path_buf());

    manager.create_vault("update-test").await.unwrap();
    let vault_path = manager.get_vault_path("update-test").unwrap();
    let vault = LocalVault::new("update-test".to_string(), vault_path);

    // Set initial value
    vault.set_secret("key", "initial").await.unwrap();
    assert_eq!(vault.get_secret("key").await.unwrap(), "initial");

    // Update value
    vault.set_secret("key", "updated").await.unwrap();
    assert_eq!(vault.get_secret("key").await.unwrap(), "updated");
}
