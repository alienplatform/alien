pub mod bindings;
pub mod commands;
pub mod lifecycle;
pub mod runner;

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
                self.ctx.cleanup().await;
            }
        }
    };
}

pub(crate) use e2e_test_context;
