//! Built-in permission sets registry
//!
//! This module provides access to the built-in permission sets that are compiled
//! into the alien-permissions crate from JSONC files at build time.
//!
//! ## How it works
//!
//! The registry is automatically generated at build time by scanning all `.jsonc` files
//! in the `permission-sets/` directory. Each JSONC file defines a permission set with
//! platform-specific permissions and binding instructions.
//!
//! ## Generation process
//!
//! 1. **Build script** (`build.rs`) runs during compilation
//! 2. **Scans** `permission-sets/` directory recursively for `.jsonc` files  
//! 3. **Parses** each file using `json5` to extract the permission set ID and content
//! 4. **Generates** Rust code that creates a static registry with all permission sets
//! 5. **Compiles** the generated code into the crate at build time
//!
//! ## Directory structure
//!
//! ```text
//! permission-sets/
//! ├── storage/
//! │   ├── data-read.jsonc
//! │   ├── data-write.jsonc
//! │   ├── management.jsonc
//! │   └── provision.jsonc
//! ├── function/
//! │   ├── execute.jsonc
//! │   ├── management.jsonc
//! │   ├── provision.jsonc
//! │   └── pull-images.jsonc
//! └── build/
//!     ├── execute.jsonc
//!     ├── management.jsonc
//!     └── provision.jsonc
//! ```
//!
//! ## Usage examples
//!
//! ```rust
//! use alien_permissions::{get_permission_set, list_permission_set_ids, has_permission_set};
//!
//! // Check if a permission set exists
//! if has_permission_set("storage/data-read") {
//!     println!("Permission set exists!");
//! }
//!
//! // Get a permission set by ID
//! if let Some(perm_set) = get_permission_set("storage/data-read") {
//!     println!("Description: {}", perm_set.description);
//!     
//!     // Access AWS permissions
//!     if let Some(aws_perms) = &perm_set.platforms.aws {
//!         for perm in aws_perms {
//!             if let Some(actions) = &perm.grant.actions {
//!                 println!("AWS actions: {:?}", actions);
//!             }
//!         }
//!     }
//! }
//!
//! // List all available permission sets
//! let all_ids = list_permission_set_ids();
//! println!("Available permission sets: {:?}", all_ids);
//! ```
//!
//! ## Adding new permission sets
//!
//! To add a new permission set:
//!
//! 1. Create a new `.jsonc` file in the appropriate subdirectory under `permission-sets/`
//! 2. Define the permission set structure following the schema in `alien-core::permissions::PermissionSet`
//! 3. Rebuild the crate - the build script will automatically include the new permission set
//!
//! Example permission set structure:
//!
//! ```jsonc
//! {
//!   "id": "my-resource/my-action",
//!   "description": "Allows performing my action on my resource",
//!   "platforms": {
//!     "aws": [
//!       {
//!         "grant": {
//!           "actions": ["myservice:MyAction"]
//!         },
//!         "binding": {
//!           "stack": {
//!             "resources": ["arn:aws:myservice:${awsRegion}:${awsAccountId}:myresource/${stackPrefix}-*"]
//!           },
//!           "resource": {
//!             "resources": ["arn:aws:myservice:${awsRegion}:${awsAccountId}:myresource/${resourceName}"]
//!           }
//!         }
//!       }
//!     ]
//!   }
//! }
//! ```
//!
//! ## Technical details
//!
//! - Permission sets are loaded into a static `HashMap` using `once_cell::sync::Lazy`
//! - JSONC parsing is done at build time using the `json5` crate
//! - Generated constants use raw string literals with `###` delimiters to avoid escaping issues
//! - The registry functions return references to static data, so there's no runtime allocation
//! - Changes to permission set files automatically trigger rebuilds via `cargo:rerun-if-changed`

// Include the generated registry code
// This includes the static PERMISSION_SETS_REGISTRY and the public API functions
include!(concat!(env!("OUT_DIR"), "/permission_sets_registry.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_contains_expected_permission_sets() {
        // Test that some known permission sets exist
        assert!(has_permission_set("storage/data-read"));
        assert!(has_permission_set("storage/data-write"));
        assert!(has_permission_set("storage/management"));
        assert!(has_permission_set("storage/provision"));
        assert!(has_permission_set("function/execute"));
        assert!(has_permission_set("function/management"));
        assert!(has_permission_set("function/provision"));
        assert!(has_permission_set("build/execute"));
        assert!(has_permission_set("build/management"));
        assert!(has_permission_set("build/provision"));
    }

    #[test]
    fn test_get_permission_set() {
        let storage_read = get_permission_set("storage/data-read");
        assert!(storage_read.is_some());

        let perm_set = storage_read.unwrap();
        assert_eq!(perm_set.id, "storage/data-read");
        assert_eq!(
            perm_set.description,
            "Allows reading data from storage buckets and containers"
        );

        // Check that it has platforms defined
        assert!(perm_set.platforms.aws.is_some());
        assert!(perm_set.platforms.gcp.is_some());
        assert!(perm_set.platforms.azure.is_some());
    }

    #[test]
    fn test_nonexistent_permission_set() {
        assert!(!has_permission_set("nonexistent/permission"));
        assert!(get_permission_set("nonexistent/permission").is_none());
    }

    #[test]
    fn test_list_permission_set_ids() {
        let ids = list_permission_set_ids();
        assert!(!ids.is_empty());
        assert!(ids.contains(&"storage/data-read"));
        assert!(ids.contains(&"function/execute"));

        // Should be sorted or at least consistent
        println!("Available permission sets: {:?}", ids);
    }

    #[test]
    fn test_permission_set_structure() {
        let function_exec = get_permission_set("function/execute").unwrap();

        // Test AWS platform
        if let Some(aws_perms) = &function_exec.platforms.aws {
            assert!(!aws_perms.is_empty());
            let first_perm = &aws_perms[0];

            // Should have actions
            assert!(first_perm.grant.actions.is_some());
            let actions = first_perm.grant.actions.as_ref().unwrap();
            assert!(actions.contains(&"logs:PutLogEvents".to_string()));

            // Should have bindings
            assert!(!first_perm.binding.is_empty());
            assert!(first_perm.binding.stack.is_some());
            assert!(first_perm.binding.resource.is_some());
        }
    }
}
