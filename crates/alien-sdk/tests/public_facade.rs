use std::collections::HashMap;

use alien_sdk::Bindings;

#[cfg(feature = "worker")]
use std::sync::Arc;

#[cfg(feature = "worker")]
use alien_sdk::worker::AlienContext;

#[cfg(feature = "worker")]
#[test]
fn alien_context_accessors_return_the_application_bindings_facade() {
    let _: for<'a> fn(&'a AlienContext) -> &'a Bindings = AlienContext::bindings;
    let _: fn(&AlienContext) -> Arc<Bindings> = AlienContext::get_bindings;
}

#[tokio::test]
async fn bindings_facade_exposes_all_application_binding_kinds() {
    let bindings =
        Bindings::from_env_map(HashMap::new()).expect("empty environment should construct");

    let storage_error = bindings
        .storage("files")
        .await
        .expect_err("missing storage binding should fail");
    let kv_error = bindings
        .kv("cache")
        .await
        .expect_err("missing KV binding should fail");
    let queue_error = bindings
        .queue("jobs")
        .await
        .expect_err("missing queue binding should fail");
    let vault_error = bindings
        .vault("secrets")
        .await
        .expect_err("missing vault binding should fail");
    let container_error = bindings
        .container("database")
        .await
        .expect_err("missing container binding should fail");

    assert_eq!(
        [
            storage_error.code.as_str(),
            kv_error.code.as_str(),
            queue_error.code.as_str(),
            vault_error.code.as_str(),
            container_error.code.as_str(),
        ],
        ["BINDING_NOT_CONFIGURED"; 5],
    );
}
