//! Test fixture for language SDKs. Production attaches these routes to the manager.
use alien_local::{LocalQueueManager, LocalQueueService};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().expect("state directory"));
    let manager = Arc::new(LocalQueueManager::new(dir.clone()));
    let service = Arc::new(LocalQueueService::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listen");
    service.register(
        "test",
        manager.clone(),
        &format!("http://{}", listener.local_addr().unwrap()),
    );
    let mut bindings = BTreeMap::new();
    for name in args {
        manager.create_queue(&name).await.expect("create queue");
        bindings.insert(
            name.clone(),
            manager.runtime_binding(&name).expect("runtime binding"),
        );
    }
    std::fs::write(
        dir.join("bindings.tmp"),
        serde_json::to_vec(&bindings).unwrap(),
    )
    .unwrap();
    std::fs::rename(dir.join("bindings.tmp"), dir.join("bindings.json")).unwrap();
    axum::serve(listener, service.router())
        .await
        .expect("serve queue fixture");
}
