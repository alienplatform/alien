use alien_core::ArtifactRegistry;

/// Create a basic ArtifactRegistry for testing
pub fn basic_artifact_registry() -> ArtifactRegistry {
    ArtifactRegistry::new("test-registry".to_string()).build()
}

/// Create an ArtifactRegistry with a custom ID
pub fn artifact_registry_with_id(id: &str) -> ArtifactRegistry {
    ArtifactRegistry::new(id.to_string()).build()
}

/// Create multiple ArtifactRegistry instances for testing
pub fn multiple_artifact_registries() -> Vec<ArtifactRegistry> {
    vec![
        artifact_registry_with_id("registry-1"),
        artifact_registry_with_id("registry-2"),
        artifact_registry_with_id("registry-3"),
    ]
}
