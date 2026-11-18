use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Build, Platform, Stack};
use std::collections::BTreeMap;

/// On Azure, every Build of a stack uses the same permission profile.
///
/// A Build's jobs are created per run, so the right to start, change and delete them is granted
/// at the stack's resource group and covers every Build job there. A holder of one Build's grant
/// can therefore start or change a job of another Build, and that job runs as the identity of its
/// own Build. With one profile, all Builds run as one identity and a holder gains nothing the
/// identity does not already have.
pub struct AzureSingleBuildProfileCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for AzureSingleBuildProfileCheck {
    fn description(&self) -> &'static str {
        "On Azure, all Builds of a stack use one permission profile"
    }

    fn should_run(&self, stack: &Stack, platform: Platform) -> bool {
        platform == Platform::Azure
            && stack
                .resources()
                .any(|(_, entry)| entry.config.downcast_ref::<Build>().is_some())
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        if platform != Platform::Azure {
            return Ok(CheckResult::success());
        }
        let mut profiles: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (id, entry) in stack.resources() {
            if let Some(build) = entry.config.downcast_ref::<Build>() {
                profiles
                    .entry(build.get_permissions().to_string())
                    .or_default()
                    .push(id.clone());
            }
        }
        if profiles.len() <= 1 {
            return Ok(CheckResult::success());
        }
        let listing = profiles
            .iter()
            .map(|(profile, builds)| format!("profile '{profile}': {}", builds.join(", ")))
            .collect::<Vec<_>>()
            .join("; ");
        Ok(CheckResult::failed(vec![format!(
            "On Azure all Builds of a stack must use one permission profile, because a build \
             grant covers every Build job in the resource group and each job runs as its \
             Build's identity. This stack's Builds use {} profiles ({listing}). Give the Builds \
             one profile, or split them into separate stacks.",
            profiles.len()
        )]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::ResourceLifecycle;

    fn stack(builds: &[(&str, &str)]) -> Stack {
        let mut stack = Stack::new("s".to_string());
        for (id, profile) in builds {
            stack = stack.add(
                Build::new(id.to_string())
                    .permissions(profile.to_string())
                    .build(),
                ResourceLifecycle::Frozen,
            );
        }
        stack.build()
    }

    #[tokio::test]
    async fn one_profile_passes_and_two_are_refused_on_azure_only() {
        let one = stack(&[("a", "build"), ("b", "build")]);
        let two = stack(&[("a", "build"), ("b", "other")]);
        let check = AzureSingleBuildProfileCheck;

        assert!(check.check(&one, Platform::Azure).await.unwrap().success);
        let refused = check.check(&two, Platform::Azure).await.unwrap();
        assert!(!refused.success);
        let text = refused.errors.join(" ");
        for part in ["'build': a", "'other': b", "one permission profile"] {
            assert!(text.contains(part), "{part}: {text}");
        }

        for platform in [Platform::Aws, Platform::Gcp] {
            assert!(!check.should_run(&two, platform), "{platform}");
            assert!(check.check(&two, platform).await.unwrap().success);
        }
        assert!(check.should_run(&two, Platform::Azure));
        assert!(!check.should_run(&stack(&[]), Platform::Azure));
    }
}
