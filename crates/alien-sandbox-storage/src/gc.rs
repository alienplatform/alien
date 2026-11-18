use std::collections::BTreeSet;

use crate::error::Result;
use crate::layout::{ObjectName, MANIFESTS_DIR, PACKS_DIR};
use crate::read::{read_head, read_manifest};
use crate::store::ObjectStore;

/// What a collection removed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GcReport {
    /// Packs deleted.
    pub packs: Vec<String>,
    /// Manifests deleted.
    pub manifests: Vec<String>,
}

/// Deletes packs and manifests HEAD no longer needs.
///
/// Only objects with a sequence number at or below HEAD's are candidates: a higher one may be
/// the live writer's commit in flight, whose HEAD put has not landed yet. Below that, a pack is
/// kept while HEAD's manifest locates a chunk in it, and only HEAD's own manifest is kept. A
/// superseded writer's leftovers carry numbers at or below HEAD's and are collected with the rest.
pub async fn collect_garbage(store: &dyn ObjectStore) -> Result<GcReport> {
    let mut report = GcReport::default();
    let Some((head, _)) = read_head(store).await? else {
        return Ok(report);
    };
    let Some(commit) = head.commit else {
        return Ok(report);
    };
    let manifest = read_manifest(store, &commit).await?;
    let live: BTreeSet<&ObjectName> = manifest.packs();
    for object in store.list(&format!("{PACKS_DIR}/")).await? {
        let Some(name) = ObjectName::parse(&object) else {
            continue;
        };
        if name.seq <= commit.seq && !live.contains(&name) {
            store.delete(&object).await?;
            report.packs.push(object);
        }
    }
    for object in store.list(&format!("{MANIFESTS_DIR}/")).await? {
        let Some(name) = ObjectName::parse(&object) else {
            continue;
        };
        if name.seq <= commit.seq && name != commit {
            store.delete(&object).await?;
            report.manifests.push(object);
        }
    }
    Ok(report)
}
