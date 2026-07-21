use super::*;

#[tokio::test]
async fn manager_access_token_refreshes_with_skew_without_forwarding_platform_token() {
    let fixture = Fixture::new(at(0), at(3600)).await;
    let provider = fixture.remote_provider().await;

    provider
        .load_storage("first")
        .await
        .expect("initial manager token should resolve");
    fixture.clock.set(at(269));
    provider
        .load_storage("second")
        .await
        .expect("manager token should remain valid before refresh-at");
    fixture.clock.set(at(270));
    provider
        .load_storage("third")
        .await
        .expect("manager token should refresh at the skew boundary");

    let manager_requests = fixture
        .manager
        .requests
        .lock()
        .expect("manager requests lock");
    assert_eq!(manager_requests.len(), 3);
    assert_eq!(
        manager_requests
            .iter()
            .map(|request| request.authorization.as_deref())
            .collect::<Vec<_>>(),
        vec![
            Some("Bearer manager-binding-token-1"),
            Some("Bearer manager-binding-token-1"),
            Some("Bearer manager-binding-token-2"),
        ]
    );
    let platform_authorization = format!("Bearer {PLATFORM_TOKEN}");
    assert!(manager_requests.iter().all(|request| {
        request.authorization.as_deref() != Some(platform_authorization.as_str())
    }));
    assert_eq!(fixture.token_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn missing_manager_token_expiry_fails_closed_before_manager_access() {
    let fixture = Fixture::new(at(0), at(3600)).await;
    fixture.set_binding_token_expiry(None);

    let error = RemoteBindingsProvider::discover_local_fixture(
        DEPLOYMENT_ID,
        PLATFORM_TOKEN,
        Some(&fixture.api_url),
        fixture.clock.clone(),
    )
    .await
    .expect_err("a manager token without expiry must not be cached");

    assert_eq!(error.code, "REMOTE_ACCESS_FAILED");
    assert_eq!(error.http_status_code, Some(502));
    assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn assignment_race_rediscovery_retries_once_against_the_new_manager() {
    let fixture = Fixture::new(at(0), at(600)).await;
    let provider = fixture.remote_provider().await;
    provider
        .load_storage("files")
        .await
        .expect("manager A should resolve the initial binding");

    let manager_b_token = Arc::new(StdRwLock::new("unminted-manager-b-token".to_string()));
    let manager_b = ManagerFixtureState {
        calls: Arc::new(AtomicUsize::new(0)),
        fail: Arc::new(AtomicBool::new(false)),
        failure_response: Arc::new(StdRwLock::new(None)),
        invalid_binding: Arc::new(AtomicBool::new(false)),
        advance_clock_to: Arc::new(StdRwLock::new(None)),
        clock: fixture.clock.clone(),
        expires_at: Arc::new(StdRwLock::new(at(3600))),
        storage_path: fixture.manager.storage_path.clone(),
        expected_token: manager_b_token.clone(),
        requests: Arc::new(StdMutex::new(Vec::new())),
    };
    let manager_b_url = spawn_manager_server(manager_b.clone()).await;
    fixture.assign_manager(MANAGER_B_ID, manager_b_url, manager_b_token);
    fixture.fail_manager_with_text(StatusCode::NOT_FOUND, "deployment moved");

    provider
        .load_storage("archive")
        .await
        .expect("404 from manager A should rediscover and retry manager B once");

    assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 2);
    assert_eq!(manager_b.calls.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.token_calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        manager_b.requests.lock().expect("manager B requests lock")[0]
            .authorization
            .as_deref(),
        Some("Bearer manager-binding-token-2")
    );
}
