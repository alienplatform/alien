use alien_core::{LifecycleRule, Storage};
use rstest::fixture;

// Test fixtures for different storage configurations

#[fixture]
pub(crate) fn basic_storage() -> Storage {
    Storage::new("basic-storage".to_string()).build()
}

#[fixture]
pub(crate) fn storage_with_versioning() -> Storage {
    Storage::new("versioned-storage".to_string())
        .versioning(true)
        .build()
}

#[fixture]
pub(crate) fn storage_with_public_read() -> Storage {
    Storage::new("public-storage".to_string())
        .public_read(true)
        .build()
}

#[fixture]
pub(crate) fn storage_with_lifecycle_rules() -> Storage {
    Storage::new("lifecycle-storage".to_string())
        .lifecycle_rules(vec![
            LifecycleRule {
                days: 30,
                prefix: Some("logs/".to_string()),
            },
            LifecycleRule {
                days: 7,
                prefix: None,
            },
        ])
        .build()
}

#[fixture]
pub(crate) fn storage_complete_config() -> Storage {
    Storage::new("complete-storage".to_string())
        .versioning(true)
        .public_read(true)
        .lifecycle_rules(vec![LifecycleRule {
            days: 90,
            prefix: Some("archive/".to_string()),
        }])
        .build()
}

#[fixture]
pub(crate) fn storage_custom_lifecycle() -> Storage {
    Storage::new("custom-lifecycle".to_string())
        .lifecycle_rules(vec![
            LifecycleRule {
                days: 1,
                prefix: Some("temp/".to_string()),
            },
            LifecycleRule {
                days: 365,
                prefix: Some("backup/".to_string()),
            },
        ])
        .build()
}

#[fixture]
pub(crate) fn storage_versioning_only() -> Storage {
    Storage::new("versioning-only".to_string())
        .versioning(true)
        .build()
}

#[fixture]
pub(crate) fn storage_public_only() -> Storage {
    Storage::new("public-only".to_string())
        .public_read(true)
        .build()
}

#[fixture]
pub(crate) fn storage_for_update_test() -> Storage {
    Storage::new("update-test".to_string())
        .versioning(false)
        .public_read(false)
        .build()
}
