//! Compute pools on Kubernetes use administrator-managed node capacity.
//!
//! Hardware sizes and machine counts are advisory. Execution requirements must
//! either be translated into Pod placement or rejected before workloads start.

use crate::{
    instance_catalog::Architecture, CapacityGroup, ComputeCluster, Container, Daemon, Stack,
};
use std::collections::{BTreeMap, HashSet};

/// Validate logical pools without querying or provisioning Kubernetes nodes.
pub fn validate_kubernetes_compute(stack: &Stack) -> Vec<String> {
    let mut errors = Vec::new();
    if let Err(message) = kubernetes_compute_architecture(stack) {
        errors.push(message);
    }
    for (_, entry) in stack.resources() {
        let Some(cluster) = entry.config.downcast_ref::<ComputeCluster>() else {
            continue;
        };
        let mut names = HashSet::new();
        if cluster.capacity_groups.is_empty() {
            errors.push(format!(
                "ComputeCluster '{}' must declare at least one pool on Kubernetes",
                cluster.id
            ));
        }
        for pool in &cluster.capacity_groups {
            if pool.group_id.is_empty() || !names.insert(pool.group_id.as_str()) {
                errors.push(format!(
                    "ComputeCluster '{}' has an empty or duplicate pool '{}'",
                    cluster.id, pool.group_id
                ));
            }
            if pool.min_size > pool.max_size {
                errors.push(format!(
                    "ComputeCluster '{}' pool '{}' has min greater than max",
                    cluster.id, pool.group_id
                ));
            }
            if pool.instance_type.is_some()
                || pool.nested_virtualization == Some(true)
                || pool
                    .profile
                    .as_ref()
                    .is_some_and(|profile| profile.gpu.is_some())
            {
                errors.push(format!("ComputeCluster '{}' pool '{}' requests instance type, GPU, or nested virtualization; Kubernetes cannot enforce these node requirements", cluster.id, pool.group_id));
            }
        }
        if cluster
            .dynamic_container_pool
            .as_ref()
            .is_some_and(|name| !names.contains(name.as_str()))
        {
            errors.push(format!(
                "ComputeCluster '{}' dynamic container pool does not exist",
                cluster.id
            ));
        }
        if !cluster.selected_failure_domains.is_empty()
            || cluster
                .container_cidr
                .as_deref()
                .is_some_and(|cidr| cidr != "10.244.0.0/16")
        {
            errors.push(format!("ComputeCluster '{}' configures provider failure domains or a container CIDR; existing Kubernetes networking and node topology are administrator-managed", cluster.id));
        }
    }
    for (_, entry) in stack.resources() {
        let placement = if let Some(container) = entry.config.downcast_ref::<Container>() {
            kubernetes_container_pool(stack, container)
        } else if let Some(daemon) = entry.config.downcast_ref::<Daemon>() {
            kubernetes_daemon_pool(stack, daemon)
        } else {
            continue;
        };
        if let Err(message) = placement {
            errors.push(message);
        }
    }
    errors
}

/// Architecture shared by source images built for this Kubernetes stack.
/// Unspecified pools and Workers inherit this build constraint, so their Pods
/// cannot land on incompatible nodes in a cluster with mixed architectures.
pub fn kubernetes_compute_architecture(stack: &Stack) -> Result<Option<Architecture>, String> {
    let mut selected = None;
    for architecture in stack
        .resources()
        .filter_map(|(_, entry)| entry.config.downcast_ref::<ComputeCluster>())
        .flat_map(|cluster| &cluster.capacity_groups)
        .filter_map(|pool| pool.profile.as_ref()?.architecture)
    {
        if selected.is_some_and(|selected| selected != architecture) {
            return Err("Kubernetes compute pools require mixed CPU architectures; one platform image cannot satisfy both".to_string());
        }
        selected = Some(architecture);
    }
    Ok(selected)
}

/// Match Pod placement to the architecture used by the stack's source images.
/// This does not associate Workers with a compute pool or reserve node capacity.
pub fn kubernetes_compute_node_selector(
    stack: &Stack,
    pool: Option<&CapacityGroup>,
) -> Result<Option<BTreeMap<String, String>>, String> {
    let built_architecture = kubernetes_compute_architecture(stack)?;
    let architecture = pool
        .and_then(|pool| pool.profile.as_ref())
        .and_then(|profile| profile.architecture)
        .or(built_architecture);
    Ok(architecture.map(|architecture| {
        BTreeMap::from([(
            "kubernetes.io/arch".to_string(),
            match architecture {
                Architecture::Arm64 => "arm64",
                Architecture::X86_64 => "amd64",
            }
            .to_string(),
        )])
    }))
}

/// Resolve placement, inferring a single declared cluster and its general or
/// sole pool. Legacy stacks without a declaration keep default scheduling.
pub fn kubernetes_container_pool<'a>(
    stack: &'a Stack,
    container: &Container,
) -> Result<Option<&'a CapacityGroup>, String> {
    kubernetes_workload_pool(
        stack,
        &container.id,
        container.cluster.as_deref(),
        container.pool.as_deref(),
    )
}

/// Resolve a daemon's pool without changing its per-node DaemonSet behavior.
pub fn kubernetes_daemon_pool<'a>(
    stack: &'a Stack,
    daemon: &Daemon,
) -> Result<Option<&'a CapacityGroup>, String> {
    kubernetes_workload_pool(
        stack,
        &daemon.id,
        daemon.cluster.as_deref(),
        daemon.pool.as_deref(),
    )
}

fn kubernetes_workload_pool<'a>(
    stack: &'a Stack,
    resource_id: &str,
    cluster: Option<&str>,
    pool: Option<&str>,
) -> Result<Option<&'a CapacityGroup>, String> {
    let cluster = match cluster {
        Some(cluster_id) => stack
            .resources
            .get(cluster_id)
            .and_then(|entry| entry.config.downcast_ref::<ComputeCluster>())
            .ok_or_else(|| {
                format!("Workload '{resource_id}' references missing ComputeCluster '{cluster_id}'")
            })?,
        None => {
            let mut clusters = stack
                .resources()
                .filter_map(|(_, entry)| entry.config.downcast_ref::<ComputeCluster>());
            let Some(cluster) = clusters.next() else {
                return if pool.is_some() {
                    Err(format!(
                        "Workload '{resource_id}' selects a pool without a ComputeCluster"
                    ))
                } else {
                    Ok(None)
                };
            };
            if clusters.next().is_some() {
                return Err(format!(
                    "Workload '{resource_id}' must select a ComputeCluster explicitly"
                ));
            }
            cluster
        }
    };
    let selected = match pool {
        Some(name) => cluster
            .capacity_groups
            .iter()
            .find(|pool| pool.group_id == name),
        None => cluster
            .capacity_groups
            .iter()
            .find(|pool| pool.group_id == "general")
            .or_else(|| (cluster.capacity_groups.len() == 1).then(|| &cluster.capacity_groups[0])),
    };
    selected.map(Some).ok_or_else(|| format!("Workload '{resource_id}' references missing or ambiguous pool '{}' in ComputeCluster '{}'", pool.unwrap_or("general"), cluster.id))
}

/// Resolve the one pool admitted for release-independent containers.
pub fn kubernetes_dynamic_pool(stack: &Stack) -> Result<Option<&CapacityGroup>, String> {
    let mut clusters = stack
        .resources()
        .filter_map(|(_, entry)| entry.config.downcast_ref::<ComputeCluster>());
    let Some(cluster) = clusters.next() else {
        return Ok(None);
    };
    if clusters.next().is_some() {
        return Err("Dynamic containers require exactly one ComputeCluster".to_string());
    }
    let name = cluster
        .dynamic_container_pool
        .as_deref()
        .unwrap_or("general");
    cluster
        .capacity_groups
        .iter()
        .find(|pool| pool.group_id == name)
        .map(Some)
        .ok_or_else(|| {
            format!(
                "ComputeCluster '{}' has no dynamic container pool '{name}'",
                cluster.id
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{instance_catalog::Architecture, ContainerCode, ResourceLifecycle};

    fn cluster() -> ComputeCluster {
        ComputeCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "apps".to_string(),
                instance_type: None,
                profile: Some(crate::MachineProfile {
                    cpu: "2".to_string(),
                    memory_bytes: 4 << 30,
                    ephemeral_storage_bytes: 20 << 30,
                    architecture: Some(Architecture::X86_64),
                    gpu: None,
                }),
                min_size: 1,
                max_size: 3,
                scale_policy: None,
                nested_virtualization: None,
            })
            .dynamic_container_pool("apps".to_string())
            .build()
    }

    fn stack(cluster: ComputeCluster) -> Stack {
        let container = Container::new("api".to_string())
            .code(ContainerCode::Image {
                image: "example.test/api:1".to_string(),
            })
            .cpu(crate::ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(crate::ResourceSpec {
                min: "128Mi".to_string(),
                desired: "128Mi".to_string(),
            })
            .permissions("default".to_string())
            .cluster("compute".to_string())
            .pool("apps".to_string())
            .build();
        Stack::new("test".to_string())
            .add(cluster, ResourceLifecycle::Frozen)
            .add(container, ResourceLifecycle::Live)
            .build()
    }

    #[test]
    fn portable_pool_preserves_placement_and_dynamic_admission() {
        let stack = stack(cluster());
        assert!(validate_kubernetes_compute(&stack).is_empty());
        let pool = kubernetes_dynamic_pool(&stack).unwrap().unwrap();
        assert_eq!(pool.group_id, "apps");
        assert_eq!(
            pool.profile.as_ref().unwrap().architecture,
            Some(Architecture::X86_64)
        );
    }

    #[test]
    fn rejects_execution_requirements_and_invalid_references() {
        let mut cluster = cluster();
        cluster.capacity_groups[0].nested_virtualization = Some(true);
        cluster.dynamic_container_pool = Some("missing".to_string());
        let errors = validate_kubernetes_compute(&stack(cluster));
        assert!(errors
            .iter()
            .any(|message| message.contains("nested virtualization")));
        assert!(errors
            .iter()
            .any(|message| message.contains("dynamic container pool")));
        let mut cluster = self::cluster();
        cluster.capacity_groups[0].group_id = "other".to_string();
        assert!(validate_kubernetes_compute(&stack(cluster))
            .iter()
            .any(|message| message.contains("pool 'apps'")));
    }

    #[test]
    fn implicit_placement_uses_the_declared_pool_architecture() {
        let stack = stack(cluster());
        let mut container = stack
            .resources
            .get("api")
            .unwrap()
            .config
            .downcast_ref::<Container>()
            .unwrap()
            .clone();
        container.cluster = None;
        container.pool = None;
        let pool = kubernetes_container_pool(&stack, &container)
            .unwrap()
            .unwrap();
        assert_eq!(pool.group_id, "apps");
        assert_eq!(
            pool.profile.as_ref().unwrap().architecture,
            Some(Architecture::X86_64)
        );
    }

    #[test]
    fn unspecified_pool_and_unpooled_worker_use_the_build_architecture() {
        let mut compute = cluster();
        let mut unspecified = compute.capacity_groups[0].clone();
        unspecified.group_id = "other".to_string();
        unspecified.profile.as_mut().unwrap().architecture = None;
        compute.capacity_groups.push(unspecified.clone());
        let stack = stack(compute);
        let expected = Some(BTreeMap::from([(
            "kubernetes.io/arch".to_string(),
            "amd64".to_string(),
        )]));
        assert_eq!(
            kubernetes_compute_node_selector(&stack, Some(&unspecified)).unwrap(),
            expected
        );
        assert_eq!(
            kubernetes_compute_node_selector(&stack, None).unwrap(),
            expected
        );
    }

    #[test]
    fn legacy_stack_keeps_default_placement() {
        let stack = Stack::new("legacy".to_string()).build();
        assert!(validate_kubernetes_compute(&stack).is_empty());
        assert!(kubernetes_dynamic_pool(&stack).unwrap().is_none());
    }
}
