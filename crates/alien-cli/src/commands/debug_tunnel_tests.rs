use super::PushTunnelGuard;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{net::TcpListener, task::AbortHandle};

struct GuardFixture {
    guard: PushTunnelGuard,
    address: SocketAddr,
    tasks: Vec<AbortHandle>,
    tokens: Vec<std::sync::Weak<str>>,
}

async fn guard_fixture(id: usize) -> GuardFixture {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fixture listener should bind");
    let address = listener
        .local_addr()
        .expect("fixture listener should expose its address");

    let writer_token: Arc<str> = format!("bearer-{id}-writer").into();
    let reader_token: Arc<str> = format!("bearer-{id}-reader").into();
    let server_token: Arc<str> = format!("bearer-{id}-server").into();
    let tokens = [&writer_token, &reader_token, &server_token]
        .map(|token| Arc::downgrade(token))
        .to_vec();

    let writer = tokio::spawn(async move {
        let _token = writer_token;
        std::future::pending::<()>().await;
    });
    let reader = tokio::spawn(async move {
        let _token = reader_token;
        std::future::pending::<()>().await;
    });
    let server = tokio::spawn(async move {
        let _listener = listener;
        let _token = server_token;
        std::future::pending::<()>().await;
    });
    let tasks = [&writer, &reader, &server]
        .map(|task| task.abort_handle())
        .to_vec();

    GuardFixture {
        guard: PushTunnelGuard::new([writer, reader, server]),
        address,
        tasks,
        tokens,
    }
}

#[tokio::test]
async fn dropping_merged_guard_aborts_every_task_and_releases_owned_state() {
    let fixtures = [guard_fixture(0).await, guard_fixture(1).await];
    let addresses = fixtures
        .iter()
        .map(|fixture| fixture.address)
        .collect::<Vec<_>>();
    let tasks = fixtures
        .iter()
        .flat_map(|fixture| fixture.tasks.iter().cloned())
        .collect::<Vec<_>>();
    let tokens = fixtures
        .iter()
        .flat_map(|fixture| fixture.tokens.iter().cloned())
        .collect::<Vec<_>>();

    for address in &addresses {
        TcpListener::bind(address)
            .await
            .expect_err("guard-owned listener should keep its port bound");
    }
    assert!(tokens.iter().all(|token| token.upgrade().is_some()));
    assert!(tasks.iter().all(|task| !task.is_finished()));

    let merged =
        PushTunnelGuard::merge(fixtures.into_iter().map(|fixture| fixture.guard).collect());
    drop(merged);

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let tasks_finished = tasks.iter().all(AbortHandle::is_finished);
            let tokens_released = tokens.iter().all(|token| token.upgrade().is_none());
            if tasks_finished && tokens_released {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("dropping the merged guard should cancel every child task");

    for address in addresses {
        TcpListener::bind(address)
            .await
            .expect("dropping the merged guard should release every listener");
    }
}
