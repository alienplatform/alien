//! Comprehensive tests for SQLite store implementations.
//!
//! Each test gets a fresh in-memory SQLite database with migrations run,
//! exercising store operations through the trait interfaces.

use std::sync::Arc;

use alien_core::{Platform, StackSettings};
use alien_manager::stores::sqlite::{
    SqliteDatabase, SqliteDeploymentStore, SqliteReleaseStore, SqliteTokenStore,
};
use alien_manager::traits::deployment_store::*;
use alien_manager::traits::release_store::*;
use alien_manager::traits::token_store::*;

/// Create a fresh database with all migrations applied.
/// Uses a temp file because WAL mode doesn't work with `:memory:` databases.
async fn fresh_db() -> Arc<SqliteDatabase> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    // Leak the tempdir so it lives for the test duration
    std::mem::forget(dir);
    Arc::new(SqliteDatabase::new(path.to_str().unwrap()).await.unwrap())
}

/// Helper to create a deployment group and return its ID.
async fn create_test_group(store: &SqliteDeploymentStore) -> String {
    let group = store
        .create_deployment_group(CreateDeploymentGroupParams {
            name: "test-group".to_string(),
            max_deployments: 100,
        })
        .await
        .unwrap();
    group.id
}

/// Helper to create a minimal deployment with defaults.
async fn create_test_deployment(
    store: &SqliteDeploymentStore,
    group_id: &str,
    name: &str,
    platform: Platform,
) -> DeploymentRecord {
    store
        .create_deployment(CreateDeploymentParams {
            name: name.to_string(),
            deployment_group_id: group_id.to_string(),
            platform,
            stack_settings: StackSettings::default(),
            environment_variables: None,
            deployment_token: None,
        })
        .await
        .unwrap()
}

// =============================================================================
// DeploymentStore tests
// =============================================================================

#[tokio::test]
async fn create_and_get_deployment() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);
    let group_id = create_test_group(&store).await;

    let created = create_test_deployment(&store, &group_id, "my-deploy", Platform::Aws).await;

    assert!(
        created.id.starts_with("ag"),
        "deployment ID should start with 'ag' prefix, got: {}",
        created.id
    );
    assert_eq!(created.name, "my-deploy");
    assert_eq!(created.deployment_group_id, group_id);
    assert_eq!(created.platform, Platform::Aws);
    assert_eq!(created.status, "pending");
    assert!(!created.retry_requested);
    assert!(created.locked_by.is_none());

    // Get by ID
    let fetched = store.get_deployment(&created.id).await.unwrap().unwrap();
    assert_eq!(fetched.id, created.id);
    assert_eq!(fetched.name, "my-deploy");
    assert_eq!(fetched.platform, Platform::Aws);
    assert_eq!(fetched.status, "pending");
}

#[tokio::test]
async fn list_by_status() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);
    let group_id = create_test_group(&store).await;

    let dep1 = create_test_deployment(&store, &group_id, "dep-1", Platform::Aws).await;
    let dep2 = create_test_deployment(&store, &group_id, "dep-2", Platform::Aws).await;
    let _dep3 = create_test_deployment(&store, &group_id, "dep-3", Platform::Aws).await;

    // Move dep1 to "running" status via set_redeploy (which sets update-pending)
    // For a cleaner test, let's use the reconcile to set status, but that's complex.
    // Instead, use set_redeploy which sets "update-pending"
    store.set_redeploy(&dep1.id).await.unwrap();
    store.set_delete_pending(&dep2.id).await.unwrap();

    // Filter by "pending" status
    let pending = store
        .list_deployments(&DeploymentFilter {
            statuses: Some(vec!["pending".to_string()]),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].name, "dep-3");

    // Filter by multiple statuses
    let mixed = store
        .list_deployments(&DeploymentFilter {
            statuses: Some(vec![
                "update-pending".to_string(),
                "delete-pending".to_string(),
            ]),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(mixed.len(), 2);
}

#[tokio::test]
async fn list_by_deployment_group() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);

    let group_a = store
        .create_deployment_group(CreateDeploymentGroupParams {
            name: "group-a".to_string(),
            max_deployments: 10,
        })
        .await
        .unwrap();

    let group_b = store
        .create_deployment_group(CreateDeploymentGroupParams {
            name: "group-b".to_string(),
            max_deployments: 10,
        })
        .await
        .unwrap();

    create_test_deployment(&store, &group_a.id, "dep-a1", Platform::Aws).await;
    create_test_deployment(&store, &group_a.id, "dep-a2", Platform::Aws).await;
    create_test_deployment(&store, &group_b.id, "dep-b1", Platform::Gcp).await;

    let group_a_deps = store
        .list_deployments(&DeploymentFilter {
            deployment_group_id: Some(group_a.id.clone()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(group_a_deps.len(), 2);

    let group_b_deps = store
        .list_deployments(&DeploymentFilter {
            deployment_group_id: Some(group_b.id.clone()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(group_b_deps.len(), 1);
    assert_eq!(group_b_deps[0].name, "dep-b1");
}

#[tokio::test]
async fn update_status() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);
    let group_id = create_test_group(&store).await;

    let dep = create_test_deployment(&store, &group_id, "dep", Platform::Aws).await;

    // set_delete_pending
    store.set_delete_pending(&dep.id).await.unwrap();
    let fetched = store.get_deployment(&dep.id).await.unwrap().unwrap();
    assert_eq!(fetched.status, "delete-pending");

    // set_delete_pending again should fail (already delete-pending)
    let result = store.set_delete_pending(&dep.id).await;
    assert!(result.is_err());

    // Create another deployment for retry and redeploy
    let dep2 = create_test_deployment(&store, &group_id, "dep2", Platform::Aws).await;

    // set_retry_requested
    store.set_retry_requested(&dep2.id).await.unwrap();
    let fetched = store.get_deployment(&dep2.id).await.unwrap().unwrap();
    assert!(fetched.retry_requested);

    // set_redeploy
    let dep3 = create_test_deployment(&store, &group_id, "dep3", Platform::Aws).await;
    store.set_redeploy(&dep3.id).await.unwrap();
    let fetched = store.get_deployment(&dep3.id).await.unwrap().unwrap();
    assert_eq!(fetched.status, "update-pending");
}

#[tokio::test]
async fn set_desired_release() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db.clone());
    let release_store = SqliteReleaseStore::new(db);
    let group_id = create_test_group(&store).await;

    // Create a deployment in "running" status (eligible for desired release)
    let dep = create_test_deployment(&store, &group_id, "dep", Platform::Aws).await;

    // Must be in an eligible status: "running", "update-failed", or "refresh-failed"
    // Use reconcile to set running, or we can just verify the SQL behavior
    // For simplicity, set_redeploy to "update-pending" first, but set_desired_release
    // only affects eligible statuses. Let's test set_deployment_desired_release instead.

    // set_deployment_desired_release works on any deployment
    let release = release_store
        .create_release(CreateReleaseParams {
            stack: alien_core::Stack::new("test-stack".to_string()).build(),
            platform: None,
            git_commit_sha: None,
            git_commit_ref: None,
            git_commit_message: None,
        })
        .await
        .unwrap();

    store
        .set_deployment_desired_release(&dep.id, &release.id)
        .await
        .unwrap();

    let fetched = store.get_deployment(&dep.id).await.unwrap().unwrap();
    assert_eq!(
        fetched.desired_release_id.as_deref(),
        Some(release.id.as_str())
    );
}

#[tokio::test]
async fn acquire_and_release() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);
    let group_id = create_test_group(&store).await;

    // Create a deployment in "pending" status (needs work)
    let dep = create_test_deployment(&store, &group_id, "dep", Platform::Aws).await;

    // Acquire should pick it up
    let acquired = store
        .acquire("session-1", &DeploymentFilter::default(), 10)
        .await
        .unwrap();

    assert_eq!(acquired.len(), 1);
    assert_eq!(acquired[0].deployment.id, dep.id);
    assert_eq!(
        acquired[0].deployment.locked_by.as_deref(),
        Some("session-1")
    );

    // Verify lock is persisted
    let fetched = store.get_deployment(&dep.id).await.unwrap().unwrap();
    assert_eq!(fetched.locked_by.as_deref(), Some("session-1"));
    assert!(fetched.locked_at.is_some());

    // Release the lock
    store.release(&dep.id, "session-1").await.unwrap();

    let fetched = store.get_deployment(&dep.id).await.unwrap().unwrap();
    assert!(fetched.locked_by.is_none());
    assert!(fetched.locked_at.is_none());
}

#[tokio::test]
async fn concurrent_acquire() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);
    let group_id = create_test_group(&store).await;

    let _dep = create_test_deployment(&store, &group_id, "dep", Platform::Aws).await;

    // First session acquires
    let acquired_1 = store
        .acquire("session-1", &DeploymentFilter::default(), 10)
        .await
        .unwrap();
    assert_eq!(acquired_1.len(), 1);

    // Second session tries to acquire - should get nothing (already locked)
    let acquired_2 = store
        .acquire("session-2", &DeploymentFilter::default(), 10)
        .await
        .unwrap();
    assert_eq!(acquired_2.len(), 0);
}

#[tokio::test]
async fn stale_lock_broken() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db.clone());
    let group_id = create_test_group(&store).await;

    let dep = create_test_deployment(&store, &group_id, "dep", Platform::Aws).await;

    // Manually set a stale lock (locked_at older than 5 minutes)
    let stale_time = "2020-01-01T00:00:00+00:00";
    let lock_sql = format!(
        "UPDATE deployments SET locked_by = 'old-session', locked_at = '{}' WHERE id = '{}'",
        stale_time, dep.id
    );
    db.conn().lock().await.execute(&lock_sql, ()).await.unwrap();

    // New session should be able to acquire (stale lock gets broken)
    let acquired = store
        .acquire("new-session", &DeploymentFilter::default(), 10)
        .await
        .unwrap();
    assert_eq!(acquired.len(), 1);
    assert_eq!(
        acquired[0].deployment.locked_by.as_deref(),
        Some("new-session")
    );
}

#[tokio::test]
async fn delete_deployment() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);
    let group_id = create_test_group(&store).await;

    let dep = create_test_deployment(&store, &group_id, "dep", Platform::Aws).await;

    // Verify it exists
    assert!(store.get_deployment(&dep.id).await.unwrap().is_some());

    // Delete it
    store.delete_deployment(&dep.id).await.unwrap();

    // Verify it's gone
    assert!(store.get_deployment(&dep.id).await.unwrap().is_none());
}

#[tokio::test]
async fn group_count_computed() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);

    let group = store
        .create_deployment_group(CreateDeploymentGroupParams {
            name: "counted-group".to_string(),
            max_deployments: 100,
        })
        .await
        .unwrap();

    // Initially 0
    let fetched = store
        .get_deployment_group(&group.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.deployment_count, 0);

    // Add two deployments
    let dep1 = create_test_deployment(&store, &group.id, "dep-1", Platform::Aws).await;
    create_test_deployment(&store, &group.id, "dep-2", Platform::Gcp).await;

    let fetched = store
        .get_deployment_group(&group.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.deployment_count, 2);

    // Delete one
    store.delete_deployment(&dep1.id).await.unwrap();

    let fetched = store
        .get_deployment_group(&group.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.deployment_count, 1);

    // Verify list_deployment_groups also computes counts
    let groups = store.list_deployment_groups().await.unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].deployment_count, 1);
}

#[tokio::test]
async fn deployment_not_found() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);

    let result = store.get_deployment("dep_nonexistent").await.unwrap();
    assert!(result.is_none());
}

// =============================================================================
// TokenStore tests
// =============================================================================

#[tokio::test]
async fn create_and_validate_token() {
    let db = fresh_db().await;
    let store = SqliteTokenStore::new(db);

    let token = store
        .create_token(CreateTokenParams {
            token_type: TokenType::Admin,
            key_prefix: "ax_admin_abc".to_string(),
            key_hash: "sha256_hash_abc".to_string(),
            deployment_group_id: None,
            deployment_id: None,
        })
        .await
        .unwrap();

    assert!(token.id.starts_with("tok_"));
    assert_eq!(token.token_type, TokenType::Admin);
    assert_eq!(token.key_prefix, "ax_admin_abc");
    assert_eq!(token.key_hash, "sha256_hash_abc");

    // Validate by hash
    let validated = store
        .validate_token("sha256_hash_abc")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(validated.id, token.id);
    assert_eq!(validated.token_type, TokenType::Admin);
}

#[tokio::test]
async fn invalid_hash_rejected() {
    let db = fresh_db().await;
    let store = SqliteTokenStore::new(db);

    store
        .create_token(CreateTokenParams {
            token_type: TokenType::Admin,
            key_prefix: "ax_admin_abc".to_string(),
            key_hash: "correct_hash".to_string(),
            deployment_group_id: None,
            deployment_id: None,
        })
        .await
        .unwrap();

    let result = store.validate_token("wrong_hash").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn list_tokens() {
    let db = fresh_db().await;
    let store = SqliteTokenStore::new(db);

    store
        .create_token(CreateTokenParams {
            token_type: TokenType::Admin,
            key_prefix: "ax_admin_1".to_string(),
            key_hash: "hash_1".to_string(),
            deployment_group_id: None,
            deployment_id: None,
        })
        .await
        .unwrap();

    store
        .create_token(CreateTokenParams {
            token_type: TokenType::DeploymentGroup,
            key_prefix: "ax_dg_2".to_string(),
            key_hash: "hash_2".to_string(),
            deployment_group_id: Some("dg_123".to_string()),
            deployment_id: None,
        })
        .await
        .unwrap();

    let tokens = store.list_tokens().await.unwrap();
    assert_eq!(tokens.len(), 2);
}

#[tokio::test]
async fn delete_token() {
    let db = fresh_db().await;
    let store = SqliteTokenStore::new(db);

    let token = store
        .create_token(CreateTokenParams {
            token_type: TokenType::Admin,
            key_prefix: "ax_admin_del".to_string(),
            key_hash: "hash_del".to_string(),
            deployment_group_id: None,
            deployment_id: None,
        })
        .await
        .unwrap();

    // Verify it exists
    assert!(store.validate_token("hash_del").await.unwrap().is_some());

    // Delete it
    store.delete_token(&token.id).await.unwrap();

    // Verify it's gone
    assert!(store.validate_token("hash_del").await.unwrap().is_none());
}

#[tokio::test]
async fn delete_nonexistent_token_errors() {
    let db = fresh_db().await;
    let store = SqliteTokenStore::new(db);

    let result = store.delete_token("tok_nonexistent").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn token_types() {
    let db = fresh_db().await;
    let store = SqliteTokenStore::new(db);

    // Admin
    let admin = store
        .create_token(CreateTokenParams {
            token_type: TokenType::Admin,
            key_prefix: "ax_admin_".to_string(),
            key_hash: "hash_admin".to_string(),
            deployment_group_id: None,
            deployment_id: None,
        })
        .await
        .unwrap();
    assert_eq!(admin.token_type, TokenType::Admin);

    // DeploymentGroup
    let dg = store
        .create_token(CreateTokenParams {
            token_type: TokenType::DeploymentGroup,
            key_prefix: "ax_dg_".to_string(),
            key_hash: "hash_dg".to_string(),
            deployment_group_id: Some("dg_123".to_string()),
            deployment_id: None,
        })
        .await
        .unwrap();
    assert_eq!(dg.token_type, TokenType::DeploymentGroup);
    assert_eq!(dg.deployment_group_id.as_deref(), Some("dg_123"));

    // Deployment
    let dep = store
        .create_token(CreateTokenParams {
            token_type: TokenType::Deployment,
            key_prefix: "ax_deploy_".to_string(),
            key_hash: "hash_dep".to_string(),
            deployment_group_id: Some("dg_123".to_string()),
            deployment_id: Some("dep_456".to_string()),
        })
        .await
        .unwrap();
    assert_eq!(dep.token_type, TokenType::Deployment);
    assert_eq!(dep.deployment_id.as_deref(), Some("dep_456"));

    // All three are round-trippable through validate
    let validated_admin = store.validate_token("hash_admin").await.unwrap().unwrap();
    assert_eq!(validated_admin.token_type, TokenType::Admin);

    let validated_dg = store.validate_token("hash_dg").await.unwrap().unwrap();
    assert_eq!(validated_dg.token_type, TokenType::DeploymentGroup);

    let validated_dep = store.validate_token("hash_dep").await.unwrap().unwrap();
    assert_eq!(validated_dep.token_type, TokenType::Deployment);
}

// =============================================================================
// ReleaseStore tests
// =============================================================================

#[tokio::test]
async fn create_and_get_release() {
    let db = fresh_db().await;
    let store = SqliteReleaseStore::new(db);

    let release = store
        .create_release(CreateReleaseParams {
            stack: alien_core::Stack::new("my-stack".to_string()).build(),
            platform: Some(Platform::Aws),
            git_commit_sha: Some("abc123".to_string()),
            git_commit_ref: Some("refs/heads/main".to_string()),
            git_commit_message: Some("Initial commit".to_string()),
        })
        .await
        .unwrap();

    assert!(release.id.starts_with("rel_"));
    assert_eq!(release.stack.id, "my-stack");
    assert_eq!(release.platform, Some(Platform::Aws));
    assert_eq!(release.git_commit_sha.as_deref(), Some("abc123"));
    assert_eq!(release.git_commit_ref.as_deref(), Some("refs/heads/main"));
    assert_eq!(
        release.git_commit_message.as_deref(),
        Some("Initial commit")
    );

    // Get by ID
    let fetched = store.get_release(&release.id).await.unwrap().unwrap();
    assert_eq!(fetched.id, release.id);
    assert_eq!(fetched.stack.id, "my-stack");
    assert_eq!(fetched.platform, Some(Platform::Aws));
}

#[tokio::test]
async fn latest_release() {
    let db = fresh_db().await;
    let store = SqliteReleaseStore::new(db);

    // No releases yet
    assert!(store.get_latest_release().await.unwrap().is_none());

    let _rel1 = store
        .create_release(CreateReleaseParams {
            stack: alien_core::Stack::new("stack-v1".to_string()).build(),
            platform: None,
            git_commit_sha: None,
            git_commit_ref: None,
            git_commit_message: Some("first".to_string()),
        })
        .await
        .unwrap();

    // Small sleep to ensure distinct timestamps
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let rel2 = store
        .create_release(CreateReleaseParams {
            stack: alien_core::Stack::new("stack-v2".to_string()).build(),
            platform: None,
            git_commit_sha: None,
            git_commit_ref: None,
            git_commit_message: Some("second".to_string()),
        })
        .await
        .unwrap();

    // Latest should be the second one
    let latest = store.get_latest_release().await.unwrap().unwrap();
    assert_eq!(latest.id, rel2.id);
    assert_eq!(latest.stack.id, "stack-v2");
}

#[tokio::test]
async fn release_not_found() {
    let db = fresh_db().await;
    let store = SqliteReleaseStore::new(db);

    let result = store.get_release("rel_nonexistent").await.unwrap();
    assert!(result.is_none());
}
