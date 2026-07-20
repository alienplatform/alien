use alien_core::{Platform, ResourceLifecycle, Stack, Storage};

pub(crate) const REMOTE_STORAGE_DATA_WRITE_PERMISSION_SET_ID: &str = "storage/remote-data-write";

/// Remote Storage is supported only for setup-owned cloud resources that opt
/// into publication. This is shared by permission derivation and validation so
/// the two preflight phases cannot disagree about which resources are exposed.
pub(crate) fn resource_ids(stack: &Stack, platform: Platform) -> Vec<String> {
    if !matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure) {
        return Vec::new();
    }

    stack
        .resources()
        .filter(|(_, entry)| {
            entry.remote_access
                && entry.lifecycle == ResourceLifecycle::Frozen
                && entry.config.downcast_ref::<Storage>().is_some()
        })
        .map(|(resource_id, _)| resource_id.clone())
        .collect()
}
