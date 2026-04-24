use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Platform, Stack};
use std::collections::{HashMap, HashSet};

/// Validates user-defined resource dependencies are valid and don't create circular references.
pub struct ValidResourceDependenciesCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for ValidResourceDependenciesCheck {
    fn description(&self) -> &'static str {
        "User-defined resource dependencies should be valid and shouldn't create circular references"
    }

    fn should_run(&self, _stack: &Stack, _platform: Platform) -> bool {
        true // Always run this check
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();

        // Build dependency graph
        let mut graph: HashMap<String, Vec<String>> = HashMap::new();

        for (resource_id, resource_entry) in stack.resources() {
            let mut dependencies = Vec::new();

            // Add explicit dependencies from resource entry
            for dep in &resource_entry.dependencies {
                dependencies.push(dep.id().to_string());
            }

            // Add implicit dependencies from resource configuration
            for dep_ref in resource_entry.config.get_dependencies() {
                dependencies.push(dep_ref.id().to_string());
            }

            graph.insert(resource_id.clone(), dependencies);
        }

        // Check for circular dependencies using DFS
        for resource_id in graph.keys() {
            if let Some(cycle) = find_cycle(&graph, resource_id) {
                errors.push(format!(
                    "Circular dependency detected: {}",
                    cycle.join(" -> ")
                ));
                break; // Only report one cycle to avoid noise
            }
        }

        if errors.is_empty() {
            Ok(CheckResult::success())
        } else {
            Ok(CheckResult::failed(errors))
        }
    }
}

/// Helper function to detect cycles in dependency graph using DFS
fn find_cycle(graph: &HashMap<String, Vec<String>>, start: &str) -> Option<Vec<String>> {
    let mut visited = HashSet::new();
    let mut path = Vec::new();
    let mut path_set = HashSet::new();

    fn dfs(
        graph: &HashMap<String, Vec<String>>,
        node: &str,
        visited: &mut HashSet<String>,
        path: &mut Vec<String>,
        path_set: &mut HashSet<String>,
    ) -> Option<Vec<String>> {
        if path_set.contains(node) {
            // Found cycle, return the cycle
            let cycle_start = path.iter().position(|x| x == node).unwrap();
            let mut cycle = path[cycle_start..].to_vec();
            cycle.push(node.to_string());
            return Some(cycle);
        }

        if visited.contains(node) {
            return None;
        }

        visited.insert(node.to_string());
        path.push(node.to_string());
        path_set.insert(node.to_string());

        if let Some(dependencies) = graph.get(node) {
            for dep in dependencies {
                if let Some(cycle) = dfs(graph, dep, visited, path, path_set) {
                    return Some(cycle);
                }
            }
        }

        path.pop();
        path_set.remove(node);
        None
    }

    dfs(graph, start, &mut visited, &mut path, &mut path_set)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        Function, FunctionCode, ResourceEntry, ResourceLifecycle, ResourceRef, Storage,
    };
    use indexmap::IndexMap;

    #[tokio::test]
    async fn test_valid_dependencies_success() {
        let storage = Storage::new("storage".to_string()).build();
        let function = Function::new("function".to_string())
            .code(FunctionCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .link(&storage) // function depends on storage - this is valid
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "storage".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(storage),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        resources.insert(
            "function".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(function),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
        };

        let check = ValidResourceDependenciesCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_circular_dependency_failure() {
        let storage = Storage::new("storage".to_string()).build();
        let function = Function::new("function".to_string())
            .code(FunctionCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .link(&storage) // function depends on storage
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "storage".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(storage),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: vec![ResourceRef::new(
                    alien_core::ResourceType::from("function"),
                    "function".to_string(),
                )], // storage depends on function - creates cycle
                remote_access: false,
            },
        );
        resources.insert(
            "function".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(function),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
        };

        let check = ValidResourceDependenciesCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("Circular dependency"));
    }
}
