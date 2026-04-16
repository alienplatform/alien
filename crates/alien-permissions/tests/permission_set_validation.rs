//! Integration tests to validate permission sets against official IAM datasets from cloud providers.
//!
//! This test validates that all permission sets defined in JSONC files contain
//! actions and permissions that actually exist in the official AWS IAM dataset,
//! GCP permissions dataset, and Azure provider operations dataset.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Cache duration in seconds (24 hours by default)
const CACHE_DURATION_SECS: u64 = 24 * 60 * 60;

/// Gets the cache directory path for IAM datasets
fn get_cache_dir() -> PathBuf {
    let cache_dir = Path::new("target").join("test_cache").join("iam_datasets");
    std::fs::create_dir_all(&cache_dir).ok(); // Create if it doesn't exist
    cache_dir
}

/// Checks if a cached file exists and is still valid (within cache duration)
fn is_cache_valid(cache_path: &Path) -> bool {
    if !cache_path.exists() {
        return false;
    }

    // Check if file is within cache duration
    if let Ok(metadata) = std::fs::metadata(cache_path) {
        if let Ok(modified) = metadata.modified() {
            if let Ok(elapsed) = modified.elapsed() {
                return elapsed < Duration::from_secs(CACHE_DURATION_SECS);
            }
        }
    }

    false
}

/// Loads cached data from file if it exists and is valid
fn load_from_cache<T>(cache_path: &Path) -> Option<T>
where
    T: for<'de> Deserialize<'de>,
{
    if !is_cache_valid(cache_path) {
        return None;
    }

    match std::fs::read_to_string(cache_path) {
        Ok(content) => match serde_json::from_str::<T>(&content) {
            Ok(data) => {
                println!("✓ Loaded from cache: {}", cache_path.display());
                Some(data)
            }
            Err(e) => {
                println!("⚠ Cache file corrupted, will re-download: {}", e);
                None
            }
        },
        Err(_) => None,
    }
}

/// Saves data to cache file
fn save_to_cache<T>(data: &T, cache_path: &Path) -> Result<()>
where
    T: Serialize,
{
    let json =
        serde_json::to_string_pretty(data).context("Failed to serialize data for caching")?;

    std::fs::write(cache_path, json)
        .with_context(|| format!("Failed to write cache file: {}", cache_path.display()))?;

    println!("✓ Cached to: {}", cache_path.display());
    Ok(())
}

/// Represents a single AWS service in the IAM dataset
#[derive(Debug, Deserialize, Serialize)]
struct AwsService {
    #[serde(rename = "prefix")]
    prefix: String,
    #[serde(rename = "privileges")]
    privileges: Vec<AwsPrivilege>,
}

/// Represents a single AWS privilege/action
#[derive(Debug, Deserialize, Serialize)]
struct AwsPrivilege {
    #[serde(rename = "privilege")]
    privilege: String,
}

/// Downloads and parses the AWS IAM dataset with caching
async fn fetch_aws_iam_dataset() -> Result<HashMap<String, HashSet<String>>> {
    let cache_path = get_cache_dir().join("aws_iam_dataset.json");

    // Try to load from cache first
    if let Some(cached_data) = load_from_cache::<HashMap<String, HashSet<String>>>(&cache_path) {
        return Ok(cached_data);
    }

    println!("Downloading AWS IAM dataset...");
    let url = "https://raw.githubusercontent.com/iann0036/iam-dataset/c8c82d0deee411fa4864cbdf99f85816a3daca64/aws/iam_definition.json";

    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .send()
        .await
        .context("Failed to download AWS IAM dataset")?;
    let body = response
        .text()
        .await
        .context("Failed to read AWS IAM dataset response")?;

    let services: Vec<AwsService> =
        serde_json::from_str(&body).context("Failed to parse AWS IAM dataset JSON")?;

    // Convert to a map of service prefix -> set of privileges for easy lookup
    let mut service_map = HashMap::new();
    for service in services {
        let privileges: HashSet<String> = service
            .privileges
            .into_iter()
            .map(|p| p.privilege)
            .collect();
        service_map.insert(service.prefix, privileges);
    }

    // Cache the result
    save_to_cache(&service_map, &cache_path).context("Failed to cache AWS IAM dataset")?;

    Ok(service_map)
}

/// Downloads and parses the GCP permissions dataset with caching
async fn fetch_gcp_permissions_dataset() -> Result<HashSet<String>> {
    let cache_path = get_cache_dir().join("gcp_permissions_dataset.json");

    // Try to load from cache first
    if let Some(cached_data) = load_from_cache::<HashSet<String>>(&cache_path) {
        return Ok(cached_data);
    }

    println!("Downloading GCP permissions dataset...");
    let url = "https://raw.githubusercontent.com/iann0036/iam-dataset/refs/heads/main/gcp/permissions.json";

    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .send()
        .await
        .context("Failed to download GCP permissions dataset")?;
    let body = response
        .text()
        .await
        .context("Failed to read GCP permissions dataset response")?;

    // The GCP dataset is a map where keys are permissions and values are arrays of roles
    // We only need the keys (permission names)
    let permissions_map: HashMap<String, serde_json::Value> =
        serde_json::from_str(&body).context("Failed to parse GCP permissions dataset JSON")?;

    // Extract just the permission names (keys)
    let permissions: HashSet<String> = permissions_map.keys().cloned().collect();

    // Cache the result
    save_to_cache(&permissions, &cache_path).context("Failed to cache GCP permissions dataset")?;

    Ok(permissions)
}

/// Represents a single Azure provider operation in the IAM dataset
#[derive(Debug, Deserialize, Serialize)]
struct AzureProviderOperation {
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    #[serde(rename = "id")]
    id: Option<String>,
    #[serde(rename = "name")]
    name: String,
    #[serde(rename = "operations")]
    operations: Vec<AzureOperation>,
    #[serde(rename = "resourceTypes")]
    resource_types: Vec<AzureResourceType>,
}

/// Represents a resource type within an Azure provider
#[derive(Debug, Deserialize, Serialize)]
struct AzureResourceType {
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    #[serde(rename = "name")]
    name: String,
    #[serde(rename = "operations")]
    operations: Vec<AzureOperation>,
}

/// Represents a single Azure operation/action
#[derive(Debug, Deserialize, Serialize)]
struct AzureOperation {
    #[serde(rename = "name")]
    name: Option<String>,
    #[serde(rename = "isDataAction")]
    is_data_action: Option<bool>,
    #[serde(rename = "description")]
    description: Option<String>,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
}

/// Downloads and parses the Azure provider operations dataset with caching
async fn fetch_azure_provider_operations_dataset() -> Result<HashMap<String, HashSet<String>>> {
    let cache_path = get_cache_dir().join("azure_provider_operations_dataset.json");

    // Try to load from cache first
    if let Some(cached_data) = load_from_cache::<HashMap<String, HashSet<String>>>(&cache_path) {
        return Ok(cached_data);
    }

    println!("Downloading Azure provider operations dataset...");
    let url = "https://raw.githubusercontent.com/iann0036/iam-dataset/refs/heads/main/azure/provider-operations.json";

    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .send()
        .await
        .context("Failed to download Azure provider operations dataset")?;
    let body = response
        .text()
        .await
        .context("Failed to read Azure provider operations dataset response")?;

    let provider_operations: Vec<AzureProviderOperation> = serde_json::from_str(&body)
        .context("Failed to parse Azure provider operations dataset JSON")?;

    // Convert to a map of service name -> set of all actions (both regular and data actions) for easy lookup
    // Use lowercase keys for case-insensitive matching
    let mut service_map = HashMap::new();
    for provider in provider_operations {
        let mut all_actions = HashSet::new();

        // Add provider-level operations
        for operation in provider.operations {
            if let Some(name) = operation.name {
                all_actions.insert(name.to_lowercase());
            }
        }

        // Add resource type operations
        for resource_type in provider.resource_types {
            for operation in resource_type.operations {
                if let Some(name) = operation.name {
                    all_actions.insert(name.to_lowercase());
                }
            }
        }

        service_map.insert(provider.name.to_lowercase(), all_actions);
    }

    // Cache the result
    save_to_cache(&service_map, &cache_path)
        .context("Failed to cache Azure provider operations dataset")?;

    Ok(service_map)
}

/// Load and parse a permission set file
fn load_permission_set(file_path: &Path) -> Result<alien_core::PermissionSet> {
    let content = fs::read_to_string(file_path).with_context(|| {
        format!(
            "Failed to read permission set file: {}",
            file_path.display()
        )
    })?;

    // Use json5 parser to handle JSONC files (JSON with comments)
    json5::from_str(&content).with_context(|| {
        format!(
            "Failed to parse permission set file: {}",
            file_path.display()
        )
    })
}

/// Validate AWS actions in a permission set
fn validate_aws_actions(
    permission_set: &alien_core::PermissionSet,
    iam_dataset: &HashMap<String, HashSet<String>>,
) -> Result<()> {
    let aws_platform_permissions = match &permission_set.platforms.aws {
        Some(platform_permissions) => platform_permissions,
        None => return Ok(()), // No AWS platform defined, skip validation
    };

    let mut all_invalid_actions = Vec::new();

    // Iterate through all platform permissions in the array
    for platform_permission in aws_platform_permissions {
        let actions = match &platform_permission.grant.actions {
            Some(actions) => actions,
            None => continue, // No actions defined for this permission, skip
        };

        let mut invalid_actions = Vec::new();

        for action in actions {
            // Extract the service and action name from the full action
            let action_parts: Vec<&str> = action.split(':').collect();
            if action_parts.len() == 2 {
                let action_service = action_parts[0];
                let action_name = action_parts[1];

                // Get the service privileges for this specific action's service
                let service_privileges = iam_dataset.get(action_service);

                match service_privileges {
                    Some(privileges) => {
                        // Check if this action exists in the service's privileges dataset
                        if !privileges.contains(action_name) {
                            invalid_actions.push(action.clone());
                        }
                    }
                    None => {
                        invalid_actions.push(format!(
                            "{} - service '{}' not found",
                            action, action_service
                        ));
                    }
                }
            } else {
                invalid_actions.push(format!("Invalid action format: {}", action));
            }
        }

        all_invalid_actions.extend(invalid_actions);
    }

    if !all_invalid_actions.is_empty() {
        anyhow::bail!(
            "Invalid AWS actions found in permission set '{}':\n{}",
            permission_set.id,
            all_invalid_actions.join("\n")
        );
    }

    println!(
        "✓ AWS actions validated for permission set '{}'",
        permission_set.id
    );
    Ok(())
}

/// Validate GCP permissions in a permission set
fn validate_gcp_permissions(
    permission_set: &alien_core::PermissionSet,
    permissions_dataset: &HashSet<String>,
) -> Result<()> {
    let gcp_platform_permissions = match &permission_set.platforms.gcp {
        Some(platform_permissions) => platform_permissions,
        None => return Ok(()), // No GCP platform defined, skip validation
    };

    let mut all_invalid_permissions = Vec::new();

    // Iterate through all platform permissions in the array
    for platform_permission in gcp_platform_permissions {
        let permissions = match &platform_permission.grant.permissions {
            Some(permissions) => permissions,
            None => continue, // No permissions defined for this permission, skip
        };

        let mut invalid_permissions = Vec::new();

        for permission in permissions {
            // Check if this permission exists in the GCP permissions dataset
            if !permissions_dataset.contains(permission) {
                invalid_permissions.push(permission.clone());
            }
        }

        all_invalid_permissions.extend(invalid_permissions);
    }

    if !all_invalid_permissions.is_empty() {
        anyhow::bail!(
            "Invalid GCP permissions found in permission set '{}':\n{}",
            permission_set.id,
            all_invalid_permissions.join("\n")
        );
    }

    println!(
        "✓ GCP permissions validated for permission set '{}'",
        permission_set.id
    );
    Ok(())
}

/// Validate Azure actions in a permission set
fn validate_azure_actions(
    permission_set: &alien_core::PermissionSet,
    provider_operations_dataset: &HashMap<String, HashSet<String>>,
) -> Result<()> {
    let azure_platform_permissions = match &permission_set.platforms.azure {
        Some(platform_permissions) => platform_permissions,
        None => return Ok(()), // No Azure platform defined, skip validation
    };

    let mut all_invalid_actions = Vec::new();

    // Iterate through all platform permissions in the array
    for platform_permission in azure_platform_permissions {
        let mut invalid_actions = Vec::new();

        // Validate regular actions
        if let Some(actions) = &platform_permission.grant.actions {
            for action in actions {
                // Extract the service name from the action (e.g., "Microsoft.Storage" from "Microsoft.Storage/storageAccounts/read")
                let action_service = action.split('/').next().unwrap_or(action);

                // Find the service operations for this specific action's service
                let service_operations =
                    provider_operations_dataset.get(&action_service.to_lowercase());

                match service_operations {
                    Some(operations) => {
                        // Check if this action exists in the service's operations dataset
                        if !operations.contains(&action.to_lowercase()) {
                            invalid_actions.push(format!("{} (regular action)", action));
                        }
                    }
                    None => {
                        invalid_actions.push(format!(
                            "{} - service '{}' not found",
                            action, action_service
                        ));
                    }
                }
            }
        }

        // Validate data actions
        if let Some(data_actions) = &platform_permission.grant.data_actions {
            for action in data_actions {
                // Extract the service name from the action (e.g., "Microsoft.Storage" from "Microsoft.Storage/storageAccounts/read")
                let action_service = action.split('/').next().unwrap_or(action);

                // Find the service operations for this specific action's service
                let service_operations =
                    provider_operations_dataset.get(&action_service.to_lowercase());

                match service_operations {
                    Some(operations) => {
                        // Check if this data action exists in the service's operations dataset
                        if !operations.contains(&action.to_lowercase()) {
                            invalid_actions.push(format!("{} (data action)", action));
                        }
                    }
                    None => {
                        invalid_actions.push(format!(
                            "{} - service '{}' not found",
                            action, action_service
                        ));
                    }
                }
            }
        }

        all_invalid_actions.extend(invalid_actions);
    }

    if !all_invalid_actions.is_empty() {
        anyhow::bail!(
            "Invalid Azure actions found in permission set '{}':\n{}",
            permission_set.id,
            all_invalid_actions.join("\n")
        );
    }

    println!(
        "✓ Azure actions validated for permission set '{}'",
        permission_set.id
    );
    Ok(())
}

/// Recursively collect all .jsonc files in the permission sets directory
fn collect_permission_set_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.exists() {
        anyhow::bail!(
            "Permission sets directory does not exist: {}",
            dir.display()
        );
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            collect_permission_set_files(&path, files)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("jsonc") {
            files.push(path);
        }
    }

    Ok(())
}

/// Tests all permission sets against the official cloud provider IAM datasets
#[tokio::test]
async fn test_permission_sets_against_iam_datasets() -> Result<()> {
    println!("Starting permission set validation against IAM datasets...");

    // Download all datasets in parallel
    let (aws_dataset, gcp_dataset, azure_dataset) = tokio::try_join!(
        fetch_aws_iam_dataset(),
        fetch_gcp_permissions_dataset(),
        fetch_azure_provider_operations_dataset(),
    )?;

    println!("All datasets loaded successfully");
    println!("AWS dataset: {} services", aws_dataset.len());
    println!("GCP dataset: {} permissions", gcp_dataset.len());
    println!("Azure dataset: {} services", azure_dataset.len());

    // Find all permission set files
    let permission_sets_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("permission-sets");

    let mut permission_set_files = Vec::new();
    collect_permission_set_files(&permission_sets_dir, &mut permission_set_files)?;

    println!("Found {} permission set files", permission_set_files.len());

    let mut validated_count = 0;
    let mut errors = Vec::new();

    for file_path in permission_set_files {
        println!("\nValidating permission set: {}", file_path.display());

        match load_permission_set(&file_path) {
            Ok(permission_set) => {
                // Validate AWS actions
                if let Err(e) = validate_aws_actions(&permission_set, &aws_dataset) {
                    errors.push(format!(
                        "AWS validation failed for {}: {}",
                        file_path.display(),
                        e
                    ));
                }

                // Validate GCP permissions
                if let Err(e) = validate_gcp_permissions(&permission_set, &gcp_dataset) {
                    errors.push(format!(
                        "GCP validation failed for {}: {}",
                        file_path.display(),
                        e
                    ));
                }

                // Validate Azure actions
                if let Err(e) = validate_azure_actions(&permission_set, &azure_dataset) {
                    errors.push(format!(
                        "Azure validation failed for {}: {}",
                        file_path.display(),
                        e
                    ));
                }

                validated_count += 1;
            }
            Err(e) => {
                errors.push(format!(
                    "Failed to load permission set {}: {}",
                    file_path.display(),
                    e
                ));
            }
        }
    }

    if !errors.is_empty() {
        anyhow::bail!(
            "Validation failed for {} permission sets:\n{}",
            errors.len(),
            errors.join("\n\n")
        );
    }

    println!(
        "\n✅ All {} permission sets validated successfully!",
        validated_count
    );
    Ok(())
}

/// Basic test to validate permission set JSON structure without network access
#[test]
fn test_permission_sets_structure() -> Result<()> {
    println!("Validating permission set JSON structure...");

    // Find all permission set files
    let permission_sets_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("permission-sets");

    let mut permission_set_files = Vec::new();
    collect_permission_set_files(&permission_sets_dir, &mut permission_set_files)?;

    println!("Found {} permission set files", permission_set_files.len());

    let mut validated_count = 0;
    let mut errors = Vec::new();

    for file_path in permission_set_files {
        println!("Validating structure: {}", file_path.display());

        match load_permission_set(&file_path) {
            Ok(permission_set) => {
                // Basic structure validation
                if permission_set.id.is_empty() {
                    errors.push(format!("{}: missing id", file_path.display()));
                }
                if permission_set.description.is_empty() {
                    errors.push(format!("{}: missing description", file_path.display()));
                }
                // Check if at least one platform is defined
                let has_any_platform = permission_set.platforms.aws.is_some()
                    || permission_set.platforms.gcp.is_some()
                    || permission_set.platforms.azure.is_some();

                if !has_any_platform {
                    errors.push(format!("{}: no platforms defined", file_path.display()));
                }

                // Check AWS platform structure
                if let Some(aws_permissions) = &permission_set.platforms.aws {
                    for (i, platform_permission) in aws_permissions.iter().enumerate() {
                        if platform_permission.binding.is_empty() {
                            errors.push(format!(
                                "{}: AWS[{}] has no bindings",
                                file_path.display(),
                                i
                            ));
                        }

                        // Check if grant has either actions, permissions, or data_actions
                        let has_actions = platform_permission
                            .grant
                            .actions
                            .as_ref()
                            .map_or(false, |v| !v.is_empty());
                        let has_permissions = platform_permission
                            .grant
                            .permissions
                            .as_ref()
                            .map_or(false, |v| !v.is_empty());
                        let has_data_actions = platform_permission
                            .grant
                            .data_actions
                            .as_ref()
                            .map_or(false, |v| !v.is_empty());

                        if !has_actions && !has_permissions && !has_data_actions {
                            errors.push(format!(
                                "{}: AWS[{}] has no grant permissions",
                                file_path.display(),
                                i
                            ));
                        }
                    }
                }

                // Check GCP platform structure
                if let Some(gcp_permissions) = &permission_set.platforms.gcp {
                    for (i, platform_permission) in gcp_permissions.iter().enumerate() {
                        if platform_permission.binding.is_empty() {
                            errors.push(format!(
                                "{}: GCP[{}] has no bindings",
                                file_path.display(),
                                i
                            ));
                        }

                        let has_actions = platform_permission
                            .grant
                            .actions
                            .as_ref()
                            .map_or(false, |v| !v.is_empty());
                        let has_permissions = platform_permission
                            .grant
                            .permissions
                            .as_ref()
                            .map_or(false, |v| !v.is_empty());
                        let has_data_actions = platform_permission
                            .grant
                            .data_actions
                            .as_ref()
                            .map_or(false, |v| !v.is_empty());

                        if !has_actions && !has_permissions && !has_data_actions {
                            errors.push(format!(
                                "{}: GCP[{}] has no grant permissions",
                                file_path.display(),
                                i
                            ));
                        }
                    }
                }

                // Check Azure platform structure
                if let Some(azure_permissions) = &permission_set.platforms.azure {
                    for (i, platform_permission) in azure_permissions.iter().enumerate() {
                        if platform_permission.binding.is_empty() {
                            errors.push(format!(
                                "{}: Azure[{}] has no bindings",
                                file_path.display(),
                                i
                            ));
                        }

                        let has_actions = platform_permission
                            .grant
                            .actions
                            .as_ref()
                            .map_or(false, |v| !v.is_empty());
                        let has_permissions = platform_permission
                            .grant
                            .permissions
                            .as_ref()
                            .map_or(false, |v| !v.is_empty());
                        let has_data_actions = platform_permission
                            .grant
                            .data_actions
                            .as_ref()
                            .map_or(false, |v| !v.is_empty());

                        if !has_actions && !has_permissions && !has_data_actions {
                            errors.push(format!(
                                "{}: Azure[{}] has no grant permissions",
                                file_path.display(),
                                i
                            ));
                        }
                    }
                }

                validated_count += 1;
            }
            Err(e) => {
                errors.push(format!(
                    "Failed to load permission set {}: {}",
                    file_path.display(),
                    e
                ));
            }
        }
    }

    if !errors.is_empty() {
        anyhow::bail!(
            "Structure validation failed for {} permission sets:\n{}",
            errors.len(),
            errors.join("\n")
        );
    }

    println!(
        "✅ All {} permission sets have valid structure!",
        validated_count
    );
    Ok(())
}
