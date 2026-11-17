use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Build, Container, Daemon, Platform, ResourceRef, Stack, Worker};
use std::collections::{BTreeMap, HashMap};

/// Refuses a workload whose links to extensions' resources would set the same environment
/// variable twice, or one the workload sets itself. A deployment would otherwise fail once it
/// reached the workload, or one link's value would hide the other's.
pub struct LinkEnvironmentCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for LinkEnvironmentCheck {
    fn description(&self) -> &'static str {
        "Links to extensions' resources set each environment variable once"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        !stack.extensions.is_empty()
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();
        for (resource_id, entry) in stack.resources() {
            let Some((links, environment)) = workload_links(&entry.config) else {
                continue;
            };
            check_workload(stack, resource_id, links, environment, &mut errors);
        }
        if errors.is_empty() {
            Ok(CheckResult::success())
        } else {
            Ok(CheckResult::failed(errors))
        }
    }
}

/// The links and own environment of a workload that gets its links' variables.
fn workload_links(
    config: &alien_core::Resource,
) -> Option<(&[ResourceRef], &HashMap<String, String>)> {
    if let Some(worker) = config.downcast_ref::<Worker>() {
        Some((&worker.links, &worker.environment))
    } else if let Some(container) = config.downcast_ref::<Container>() {
        Some((&container.links, &container.environment))
    } else if let Some(daemon) = config.downcast_ref::<Daemon>() {
        Some((&daemon.links, &daemon.environment))
    } else {
        config
            .downcast_ref::<Build>()
            .map(|build| (build.links.as_slice(), &build.environment))
    }
}

fn check_workload(
    stack: &Stack,
    resource_id: &str,
    links: &[ResourceRef],
    environment: &HashMap<String, String>,
    errors: &mut Vec<String>,
) {
    // Each name, with the first link that sets it.
    let mut set_by: BTreeMap<&str, &str> = BTreeMap::new();
    for link in links {
        let Some(descriptor) = stack.descriptor(link.resource_type()) else {
            continue;
        };
        for name in &descriptor.link.env {
            if let Some(problem) = alien_core::link_env_name_problem(name) {
                errors.push(format!(
                    "The link from '{resource_id}' to '{}' sets {problem}",
                    link.id()
                ));
                continue;
            }
            if environment.contains_key(name) {
                errors.push(format!(
                    "The link from '{resource_id}' to '{}' sets `{name}`, which '{resource_id}' sets in its own environment. Rename the variable in the workload's environment",
                    link.id()
                ));
            }
            if let Some(first) = set_by.insert(name, link.id()) {
                errors.push(format!(
                    "'{resource_id}' links '{first}' and '{}', and both links set `{name}`. Link one of them from another workload",
                    link.id()
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{ResourceLifecycle, ResourceType};

    fn manifest(env: &[&str]) -> alien_core::ExtensionManifest {
        serde_json::from_value(serde_json::json!({
            "name": "acme",
            "version": "0.1.0",
            "digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            "resourceTypes": {
                "acme/dns-zone": {
                    "lifecycles": ["frozen", "live"],
                    "holdsData": false,
                    "setup": {"cloudformation": [], "terraform": [], "helm": [], "importers": []},
                    "controllerPlatforms": [],
                    "permissionSets": [],
                    "link": {"permissionSets": [], "env": env},
                    "schemas": {"config": {}, "outputs": {}}
                }
            }
        }))
        .unwrap()
    }

    fn zone(id: &str) -> ResourceRef {
        ResourceRef::new(ResourceType::from("acme/dns-zone"), id.to_string())
    }

    fn stack(env: &[&str], links: &[&str], own: &[&str]) -> Stack {
        let mut worker = Worker::new("api".to_string())
            .code(alien_core::WorkerCode::Image {
                image: "example:latest".to_string(),
            })
            .permissions("execution".to_string())
            .build();
        worker.links = links.iter().map(|id| zone(id)).collect();
        worker.environment = own
            .iter()
            .map(|name| (name.to_string(), "value".to_string()))
            .collect();
        let mut stack = Stack::new("app".to_string())
            .add(worker, ResourceLifecycle::Live)
            .build();
        stack.extensions = vec![manifest(env)];
        stack
    }

    async fn errors(stack: Stack) -> Vec<String> {
        LinkEnvironmentCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap()
            .errors
    }

    #[tokio::test]
    async fn one_link_per_name_passes() {
        assert!(errors(stack(&["ACME_ZONE_ID"], &["zone"], &[]))
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn two_links_that_set_the_same_name_are_refused() {
        let errors = errors(stack(&["ACME_ZONE_ID"], &["setup-zone", "live-zone"], &[])).await;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].contains("'setup-zone' and 'live-zone'"),
            "{errors:?}"
        );
    }

    #[tokio::test]
    async fn a_link_that_sets_the_workload_s_own_variable_is_refused() {
        let errors = errors(stack(&["ACME_ZONE_ID"], &["zone"], &["ACME_ZONE_ID"])).await;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("its own environment"), "{errors:?}");
    }

    #[tokio::test]
    async fn a_reserved_name_is_refused() {
        let errors = errors(stack(&["ALIEN_ZONE"], &["zone"], &[])).await;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("reserves"), "{errors:?}");
    }
}
