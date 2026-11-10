//! Redis persistence needs high availability on the clouds.
//!
//! The clouds only keep data across restarts and maintenance when the server has a
//! replica: ElastiCache restores from a replica and snapshots, Memorystore writes RDB
//! files from a replicated instance, and Azure Managed Redis rejects persistence
//! without HA. Local runs one process with an append-only file, so it is exempt.

use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Platform, Redis, Stack};

pub struct RedisPersistenceCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for RedisPersistenceCheck {
    fn description(&self) -> &'static str {
        "Redis persistence requires high availability on AWS, GCP and Azure"
    }

    fn should_run(&self, stack: &Stack, platform: Platform) -> bool {
        matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure)
            && stack
                .resources
                .values()
                .any(|r| r.config.downcast_ref::<Redis>().is_some())
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let mut result = CheckResult::success();
        for (id, entry) in &stack.resources {
            let Some(redis) = entry.config.downcast_ref::<Redis>() else {
                continue;
            };
            if redis.persistence && !redis.high_availability {
                result.add_error(format!(
                    "Redis '{id}': persistence needs highAvailability(): the cloud only keeps \
                     data across restarts and maintenance when the server has a replica. Add \
                     .highAvailability() to keep the data, or remove .persistence() to run it \
                     as a cache."
                ));
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{ResourceEntry, ResourceLifecycle};

    fn stack(persistence: bool, high_availability: bool) -> Stack {
        let mut redis = Redis::new("cache".to_string()).build();
        redis.persistence = persistence;
        redis.high_availability = high_availability;
        Stack::new("t".to_string())
            .add(redis, ResourceLifecycle::Live)
            .build()
    }

    #[tokio::test]
    async fn persistence_without_ha_is_rejected_on_every_cloud() {
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let s = stack(true, false);
            assert!(RedisPersistenceCheck.should_run(&s, platform));
            let result = RedisPersistenceCheck.check(&s, platform).await.unwrap();
            assert!(!result.success, "{platform:?}");
            assert!(result.errors[0].contains("persistence needs highAvailability()"));
        }
    }

    #[tokio::test]
    async fn cache_and_persistent_ha_pass_and_local_is_exempt() {
        for (p, ha) in [(false, false), (false, true), (true, true)] {
            let result = RedisPersistenceCheck
                .check(&stack(p, ha), Platform::Aws)
                .await
                .unwrap();
            assert!(result.success);
        }
        assert!(!RedisPersistenceCheck.should_run(&stack(true, false), Platform::Local));
    }
}
