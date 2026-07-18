pub mod bindings;
pub mod commands;
pub mod container;
pub mod events;
pub mod lifecycle;
pub mod routing;
pub mod runner;
pub mod runtime_less;

use std::future::Future;
use std::time::Duration;

/// Poll `attempt` every `interval` until it yields `Some(value)` or `timeout`
/// elapses. An `Err` from `attempt` aborts immediately (hard failure);
/// `Ok(None)` means "not ready yet, keep polling". Returns `Ok(None)` on
/// timeout so the caller can build its own context-rich error.
#[allow(dead_code)]
pub async fn poll_until<T, F, Fut>(
    timeout: Duration,
    interval: Duration,
    mut attempt: F,
) -> anyhow::Result<Option<T>>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = anyhow::Result<Option<T>>>,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let Some(value) = attempt().await? {
            return Ok(Some(value));
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(None);
        }
        tokio::time::sleep(interval).await;
    }
}

/// Fetch `field` from a JSON object and require it to be a nonempty string.
#[allow(dead_code)]
pub fn require_nonempty_str<'a>(
    value: &'a serde_json::Value,
    field: &str,
    label: &str,
) -> anyhow::Result<&'a str> {
    let s = value.get(field).and_then(|v| v.as_str()).unwrap_or("");
    if s.is_empty() {
        anyhow::bail!("{label} missing {field}: {value:?}");
    }
    Ok(s)
}

/// Define a test-context wrapper struct that runs `e2e::setup()` on setup
/// and `TestContext::cleanup()` on teardown (even on panic).
macro_rules! e2e_test_context {
    ($name:ident, $platform:expr, $model:expr, $lang:expr) => {
        struct $name {
            ctx: alien_test::TestContext,
        }

        impl test_context::AsyncTestContext for $name {
            async fn setup() -> Self {
                alien_test::e2e::init_tracing();
                let ctx = alien_test::e2e::setup($platform, $model, $lang)
                    .await
                    .expect(concat!(stringify!($name), " setup failed"));
                Self { ctx }
            }

            async fn teardown(self) {
                self.ctx
                    .cleanup()
                    .await
                    .expect("E2E cleanup must reach a safe terminal state");
            }
        }
    };
}

pub(crate) use e2e_test_context;
