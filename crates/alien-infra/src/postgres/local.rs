pub use alien_infra_local::postgres::*;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use alien_core::{Platform, Postgres};

    use crate::core::{controller_test::SingleControllerExecutor, MockPlatformServiceProvider};

    use super::{LocalPostgresController, LocalPostgresState};

    fn local_postgres(version: &str) -> Postgres {
        Postgres::new("db".to_string())
            .version(version.to_string())
            .build()
    }

    async fn ready_executor() -> SingleControllerExecutor {
        SingleControllerExecutor::builder()
            .resource(local_postgres("17"))
            .controller(LocalPostgresController::mock_ready("db", 5432))
            .platform(Platform::Local)
            .service_provider(Arc::new(MockPlatformServiceProvider::new()))
            .with_test_dependencies()
            .build()
            .await
            .expect("executor should build")
    }

    #[tokio::test]
    async fn update_rejects_version_change() {
        let mut executor = ready_executor().await;
        executor
            .update(local_postgres("16"))
            .expect("transition to update");
        let error = executor.step().await.expect_err(
            "a Local version change must fail loud (no pg_upgrade), not be reported as applied",
        );
        assert_eq!(error.code, "RESOURCE_CONFIG_INVALID");
    }

    #[tokio::test]
    async fn update_is_noop_on_cpu_memory_change() {
        let mut executor = ready_executor().await;
        let mut resized = local_postgres("17");
        resized.cpu = Some("4".to_string());
        resized.memory = Some("8Gi".to_string());
        executor.update(resized).expect("transition to update");
        executor
            .step()
            .await
            .expect("a cpu/memory-only change must be a clean no-op on Local");
        assert_eq!(
            executor
                .internal_state::<LocalPostgresController>()
                .expect("controller should be LocalPostgresController")
                .state(),
            &LocalPostgresState::Ready,
        );
    }
}
