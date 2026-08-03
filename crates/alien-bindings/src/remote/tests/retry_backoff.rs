use super::*;

#[tokio::test]
async fn concurrent_failed_refresh_is_single_flight_while_cache_is_unexpired() {
    let fixture = Fixture::new(at(0), at(600)).await;
    let provider = fixture.remote_provider().await;
    let bindings = RemoteBindings::from_provider(provider);
    let storage = bindings
        .storage("files")
        .await
        .expect("initial remote Storage resolution");
    storage
        .put(&Path::from("shared.txt"), PutPayload::from_static(b"value"))
        .await
        .expect("seed fixture object");

    fixture.manager.fail.store(true, Ordering::SeqCst);
    fixture.clock.set(at(481));
    let operations = (0..16).map(|_| {
        let storage = storage.clone();
        async move { storage.head(&Path::from("shared.txt")).await }
    });
    let results = join_all(operations).await;

    assert!(results.iter().all(|result| result.is_ok()));
    assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 2);

    for _ in 0..8 {
        storage
            .head(&Path::from("shared.txt"))
            .await
            .expect("cached lease should remain usable during retry cooldown");
    }
    assert_eq!(
        fixture.manager.calls.load(Ordering::SeqCst),
        2,
        "sequential operations must not hammer the manager after the failed flight"
    );
}

#[tokio::test]
async fn retryable_refresh_failures_back_off_then_recover_on_the_same_handle() {
    let fixture = Fixture::new(at(0), at(600)).await;
    let provider = fixture.remote_provider().await;
    let bindings = RemoteBindings::from_provider(provider);
    let storage = bindings
        .storage("files")
        .await
        .expect("initial remote Storage resolution");
    storage
        .put(&Path::from("shared.txt"), PutPayload::from_static(b"value"))
        .await
        .expect("seed fixture object");

    fixture.manager.fail.store(true, Ordering::SeqCst);
    fixture.clock.set(at(481));
    storage
        .head(&Path::from("shared.txt"))
        .await
        .expect("first failed refresh should use the unexpired lease");
    assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 2);

    fixture.clock.set(at(485));
    storage
        .head(&Path::from("shared.txt"))
        .await
        .expect("the first five-second cooldown should use the cached lease");
    assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 2);

    fixture.clock.set(at(486));
    storage
        .head(&Path::from("shared.txt"))
        .await
        .expect("the second failed refresh should still use the cached lease");
    assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 3);

    fixture.clock.set(at(495));
    storage
        .head(&Path::from("shared.txt"))
        .await
        .expect("the doubled cooldown should use the cached lease");
    assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 3);

    fixture.manager.fail.store(false, Ordering::SeqCst);
    fixture.set_manager_expiry(at(3600));
    fixture.clock.set(at(496));
    storage
        .head(&Path::from("shared.txt"))
        .await
        .expect("the existing handle should recover when the cooldown elapses");
    assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 4);

    storage
        .head(&Path::from("shared.txt"))
        .await
        .expect("the recovered lease should be cached");
    assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn unstructured_rate_limit_uses_unexpired_cache_during_backoff() {
    let fixture = Fixture::new(at(0), at(600)).await;
    let provider = fixture.remote_provider().await;
    let bindings = RemoteBindings::from_provider(provider);
    let storage = bindings
        .storage("files")
        .await
        .expect("initial remote Storage resolution");
    storage
        .put(
            &Path::from("rate-limited.txt"),
            PutPayload::from_static(b"value"),
        )
        .await
        .expect("seed fixture object");

    fixture.fail_manager_with(
        StatusCode::TOO_MANY_REQUESTS,
        serde_json::json!("rate limited"),
    );
    fixture.clock.set(at(481));
    storage
        .head(&Path::from("rate-limited.txt"))
        .await
        .expect("unstructured rate limit should use the unexpired cached lease");
    assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 2);

    fixture.clock.set(at(485));
    storage
        .head(&Path::from("rate-limited.txt"))
        .await
        .expect("rate-limit cooldown should continue using the cached lease");
    assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn malformed_server_error_uses_unexpired_cache_during_backoff() {
    let fixture = Fixture::new(at(0), at(600)).await;
    let provider = fixture.remote_provider().await;
    let bindings = RemoteBindings::from_provider(provider);
    let storage = bindings
        .storage("files")
        .await
        .expect("initial remote Storage resolution");
    storage
        .put(
            &Path::from("server-error.txt"),
            PutPayload::from_static(b"value"),
        )
        .await
        .expect("seed fixture object");

    fixture.fail_manager_with_text(
        StatusCode::INTERNAL_SERVER_ERROR,
        "<html>upstream exploded</html>",
    );
    fixture.clock.set(at(481));
    storage
        .head(&Path::from("server-error.txt"))
        .await
        .expect("malformed server error should use the unexpired cached lease");
    assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 2);

    fixture.clock.set(at(485));
    storage
        .head(&Path::from("server-error.txt"))
        .await
        .expect("server-error cooldown should continue using the cached lease");
    assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 2);
}

#[test]
fn refresh_retry_delay_is_exponential_and_bounded() {
    assert_eq!(refresh_retry_delay(1), ChronoDuration::seconds(5));
    assert_eq!(refresh_retry_delay(2), ChronoDuration::seconds(10));
    assert_eq!(refresh_retry_delay(3), ChronoDuration::seconds(20));
    assert_eq!(refresh_retry_delay(4), ChronoDuration::seconds(30));
    assert_eq!(refresh_retry_delay(100), ChronoDuration::seconds(30));
}

#[tokio::test]
async fn malformed_manager_response_does_not_poison_the_cache() {
    let fixture = Fixture::new(at(0), at(600)).await;
    fixture
        .manager
        .invalid_binding
        .store(true, Ordering::SeqCst);
    let provider = fixture.remote_provider().await;
    let bindings = RemoteBindings::from_provider(provider);

    bindings
        .storage("files")
        .await
        .expect_err("invalid binding must fail before caching");
    fixture
        .manager
        .invalid_binding
        .store(false, Ordering::SeqCst);
    fixture.clock.set(at(INITIAL_REFRESH_RETRY_DELAY_SECONDS));
    bindings
        .storage("files")
        .await
        .expect("a valid response must be retried and cached after the cooldown");

    assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn serves_unexpired_cache_on_refresh_failure_then_fails_closed_at_expiry() {
    let fixture = Fixture::new(at(0), at(600)).await;
    let provider = fixture.remote_provider().await;
    let bindings = RemoteBindings::from_provider(provider);
    let storage = bindings
        .storage("files")
        .await
        .expect("initial remote Storage resolution");
    storage
        .put(&Path::from("lease.txt"), PutPayload::from_static(b"valid"))
        .await
        .expect("seed fixture object");

    fixture.manager.fail.store(true, Ordering::SeqCst);
    fixture.clock.set(at(599));
    let metadata = storage
        .head(&Path::from("lease.txt"))
        .await
        .expect("unexpired lease should survive failed refresh");
    assert_eq!(metadata.size, 5);
    assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 2);

    fixture.clock.set(at(600));
    let error = storage
        .head(&Path::from("lease.txt"))
        .await
        .expect_err("lease expiry must cap cooldown and retry before failing closed");
    assert!(error.to_string().contains("Remote access failed"));
    assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 3);
}
