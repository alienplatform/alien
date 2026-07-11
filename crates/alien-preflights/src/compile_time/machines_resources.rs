use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Build, Container, Platform, ServiceAccount, Stack, Worker};

pub struct MachinesResourcesCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for MachinesResourcesCheck {
    fn code(&self) -> Option<&'static str> {
        Some("MACHINES_UNSUPPORTED_RESOURCE")
    }

    fn description(&self) -> &'static str {
        "Machines deployments support only Horizon-schedulable resources"
    }

    fn should_run(&self, stack: &Stack, platform: Platform) -> bool {
        platform == Platform::Machines && stack.resources().next().is_some()
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();

        for (resource_id, entry) in stack.resources() {
            if entry.config.downcast_ref::<Worker>().is_some() {
                errors.push(format!(
                    "Worker '{resource_id}' is not supported on platform 'machines'. Workers are supported on aws, gcp, azure, kubernetes, and local. Use a daemon or stateless replicated container for machines deployments."
                ));
            }

            if entry.config.downcast_ref::<Build>().is_some() {
                errors.push(format!(
                    "Build '{resource_id}' is not supported on platform 'machines'. Builds are supported on aws, gcp, azure, and kubernetes."
                ));
            }

            if entry.config.downcast_ref::<ServiceAccount>().is_some() {
                errors.push(format!(
                    "ServiceAccount '{resource_id}' is not supported on platform 'machines'. Service accounts are supported on aws, gcp, azure, kubernetes, and local."
                ));
            }

            if let Some(container) = entry.config.downcast_ref::<Container>() {
                if container.stateful {
                    errors.push(format!(
                        "Stateful container '{resource_id}' is not supported on platform 'machines'. Stateful containers are supported on kubernetes. Use a stateless replicated container for machines deployments."
                    ));
                }
            }
        }

        if errors.is_empty() {
            Ok(CheckResult::success())
        } else {
            Ok(CheckResult::failed(errors))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        ContainerCode, Resource, ResourceEntry, ResourceLifecycle, ResourceSpec, WorkerCode,
    };
    use indexmap::IndexMap;

    fn stack(resources: IndexMap<String, ResourceEntry>) -> Stack {
        Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: Default::default(),
            supported_platforms: None,
            inputs: vec![],
        }
    }

    fn entry(resource: Resource) -> ResourceEntry {
        ResourceEntry {
            config: resource,
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
        }
    }

    fn stateless_container(id: &str) -> Container {
        Container::new(id.to_string())
            .code(ContainerCode::Image {
                image: "test:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("default".to_string())
            .build()
    }

    #[tokio::test]
    async fn stateless_container_passes_on_machines() {
        let mut resources = IndexMap::new();
        resources.insert(
            "web".to_string(),
            entry(Resource::new(stateless_container("web"))),
        );

        let result = MachinesResourcesCheck
            .check(&stack(resources), Platform::Machines)
            .await
            .expect("check should run");

        assert!(result.success);
        assert!(result.errors.is_empty());
    }

    #[tokio::test]
    async fn worker_fails_with_exact_code() {
        let worker = Worker::new("job".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("default".to_string())
            .build();
        let mut resources = IndexMap::new();
        resources.insert("job".to_string(), entry(Resource::new(worker)));

        let result = MachinesResourcesCheck
            .check(&stack(resources), Platform::Machines)
            .await
            .expect("check should run");

        assert!(!result.success);
        assert_eq!(
            MachinesResourcesCheck.code(),
            Some("MACHINES_UNSUPPORTED_RESOURCE")
        );
        assert_eq!(
            result.errors,
            vec![
                "Worker 'job' is not supported on platform 'machines'. Workers are supported on aws, gcp, azure, kubernetes, and local. Use a daemon or stateless replicated container for machines deployments."
                    .to_string()
            ]
        );
    }

    #[tokio::test]
    async fn build_and_service_account_fail_on_machines() {
        let build = Build::new("image-build".to_string())
            .permissions("default".to_string())
            .build();
        let service_account = ServiceAccount::new("runtime".to_string()).build();
        let mut resources = IndexMap::new();
        resources.insert("image-build".to_string(), entry(Resource::new(build)));
        resources.insert("runtime".to_string(), entry(Resource::new(service_account)));

        let result = MachinesResourcesCheck
            .check(&stack(resources), Platform::Machines)
            .await
            .expect("check should run");

        assert!(!result.success);
        assert_eq!(result.errors.len(), 2);
        assert!(result.errors[0].starts_with("Build 'image-build'"));
        assert!(result.errors[1].starts_with("ServiceAccount 'runtime'"));
    }

    #[tokio::test]
    async fn stateful_container_fails_on_machines() {
        let mut container = stateless_container("db");
        container.stateful = true;
        let mut resources = IndexMap::new();
        resources.insert("db".to_string(), entry(Resource::new(container)));

        let result = MachinesResourcesCheck
            .check(&stack(resources), Platform::Machines)
            .await
            .expect("check should run");

        assert!(!result.success);
        assert_eq!(
            result.errors,
            vec![
                "Stateful container 'db' is not supported on platform 'machines'. Stateful containers are supported on kubernetes. Use a stateless replicated container for machines deployments."
                    .to_string()
            ]
        );
    }
}
