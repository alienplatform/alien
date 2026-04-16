use alien_core::{Build, ComputeType};
use rstest::fixture;
use std::collections::HashMap;

#[fixture]
pub fn basic_build() -> Build {
    Build::new("basic-build".to_string())
        .permissions("default-profile".to_string())
        .build()
}

#[fixture]
pub fn build_with_env_vars() -> Build {
    let mut env = HashMap::new();
    env.insert("NODE_ENV".to_string(), "production".to_string());
    env.insert("API_KEY".to_string(), "secret123".to_string());
    env.insert("BUILD_VERSION".to_string(), "1.0.0".to_string());

    Build::new("env-build".to_string())
        .environment(env)
        .permissions("default-profile".to_string())
        .build()
}

#[fixture]
pub fn build_medium_compute() -> Build {
    Build::new("medium-build".to_string())
        .compute_type(ComputeType::Medium)
        .permissions("default-profile".to_string())
        .build()
}

#[fixture]
pub fn build_custom_image() -> Build {
    Build::new("custom-image-build".to_string())
        .permissions("default-profile".to_string())
        .build()
}

#[fixture]
pub fn build_with_storage_link() -> Build {
    let dummy_storage = alien_core::Storage::new("build-artifacts".to_string()).build();

    Build::new("build-with-storage".to_string())
        .link(&dummy_storage)
        .permissions("default-profile".to_string())
        .build()
}

#[fixture]
pub fn build_small_compute() -> Build {
    Build::new("small-build".to_string())
        .compute_type(ComputeType::Small)
        .permissions("default-profile".to_string())
        .build()
}

#[fixture]
pub fn build_docker() -> Build {
    let mut env = HashMap::new();
    env.insert(
        "BUILD_VERSION".to_string(),
        "$(date +%Y%m%d-%H%M%S)".to_string(),
    );

    Build::new("docker-build".to_string())
        .environment(env)
        .compute_type(ComputeType::Large)
        .permissions("default-profile".to_string())
        .build()
}
