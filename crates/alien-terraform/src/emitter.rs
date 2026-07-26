//! Per-resource Terraform emitter trait.
//!
//! Emitters return `hcl::Block` / `hcl::Expression` from `hcl-rs` directly \u2014
//! there is no intermediate IR. The crate-level generator merges the emitted
//! `TfFragment`s into a single module body and runs it through the `hcl-rs`
//! formatter.
//!
//! Plugins extend the surface by registering additional implementations
//! against a [`crate::TfRegistry`]. Built-ins layer the same way (see
//! [`crate::TfRegistry::built_in`]).

use crate::registry::TfRegistry;
use alien_core::{import::EmitContext, Result};
use hcl::{expr::Expression, structure::Block};
use indexmap::IndexMap;

/// Terraform fragment emitted by a single `(resource_type, platform)` emitter.
#[derive(Debug, Default)]
pub struct TfFragment {
    /// `resource "..." "..." { ... }` blocks. Merged into `main.tf`.
    pub resource_blocks: Vec<Block>,
    /// `data "..." "..." { ... }` blocks. Merged into `main.tf`.
    pub data_blocks: Vec<Block>,
    /// Extra `locals { ... }` entries the emitter contributed. Merged across
    /// all emitters into a single `locals` block in `main.tf`.
    pub locals: IndexMap<String, Expression>,
    /// Addresses (`type`, `label`) of blocks in [`Self::resource_blocks`] that
    /// are shared support infrastructure rather than owned by this fragment's
    /// resource — a project-wide custom role several resources reference.
    /// Shared blocks outlive any single resource: the gating post-pass never
    /// gates them, and the generator deduplicates body-identical copies across
    /// fragments. Keyed by address, not index, so removals and reordering in
    /// later passes cannot desynchronize the classification.
    shared_addresses: std::collections::HashSet<(String, String)>,
    /// Blocks that carry ANOTHER resource's gate: a grant rendered on behalf
    /// of a resource the deployer can decline, emitted by a shared emitter
    /// (management role, service account) that is itself never gated. The
    /// generator installs the count and records the address, so these blocks
    /// land inside the same reference rewrite and rendered-output scan as
    /// every other gated block. Address-keyed for the same reason
    /// [`Self::shared_addresses`] is.
    ///
    /// Several inputs on one block means the block is one cloud object merged
    /// from several contributors, and it exists while ANY of them is enabled.
    gated_contributions: std::collections::HashMap<(String, String), Vec<String>>,
}

impl TfFragment {
    /// Empty fragment (used by emitters that only contribute via
    /// [`Self::locals`]).
    pub fn empty() -> Self {
        Self::default()
    }

    /// Builder helper.
    pub fn with_resource(mut self, block: Block) -> Self {
        self.resource_blocks.push(block);
        self
    }

    /// Builder helper.
    pub fn with_data(mut self, block: Block) -> Self {
        self.data_blocks.push(block);
        self
    }

    /// Builder helper.
    pub fn with_local(mut self, name: impl Into<String>, value: Expression) -> Self {
        self.locals.insert(name.into(), value);
        self
    }

    /// Append a resource block classified as shared support infrastructure.
    /// It renders in place like any other block, but the gating post-pass
    /// leaves it ungated even when this fragment's resource is gated.
    pub fn push_shared_resource(&mut self, block: Block) {
        if let Some(address) = block_address(&block) {
            self.shared_addresses.insert(address);
        }
        self.resource_blocks.push(block);
    }

    /// Append a resource block that exists on behalf of gated resources, and
    /// declare whose gates it carries. An empty list appends it unchanged.
    ///
    /// The block is rendered plain here — the generator installs the count, so
    /// it goes through the same checks and the same address bookkeeping as a
    /// gated resource's own blocks.
    pub fn push_gated_resource(&mut self, block: Block, input_ids: &[String]) {
        if !input_ids.is_empty() {
            if let Some(address) = block_address(&block) {
                self.declare_contribution(address, input_ids);
            }
        }
        self.resource_blocks.push(block);
    }

    /// Declare that every non-shared block appended since `from_index` carries
    /// `input_id`'s gate. `None` declares nothing, so an ungated resource can
    /// call the same shared emitters without threading a gate through them.
    pub fn mark_gated_from(&mut self, from_index: usize, input_id: Option<&str>) {
        let Some(input_id) = input_id else {
            return;
        };
        let addresses: Vec<(String, String)> = self.resource_blocks[from_index..]
            .iter()
            .filter(|block| !self.is_shared(block))
            .filter_map(block_address)
            .collect();
        for address in addresses {
            self.declare_contribution(address, std::slice::from_ref(&input_id.to_string()));
        }
    }

    fn declare_contribution(&mut self, address: (String, String), input_ids: &[String]) {
        let gates = self.gated_contributions.entry(address).or_default();
        for input_id in input_ids {
            if !gates.contains(input_id) {
                gates.push(input_id.clone());
            }
        }
    }

    /// The gates a contributed block carries, `None` when it is not one.
    pub fn contribution_gates(&self, block: &Block) -> Option<&[String]> {
        block_address(block)
            .and_then(|address| self.gated_contributions.get(&address))
            .map(Vec::as_slice)
    }

    /// Whether a block of this fragment is shared support infrastructure.
    pub fn is_shared(&self, block: &Block) -> bool {
        block_address(block)
            .map(|address| self.shared_addresses.contains(&address))
            .unwrap_or(false)
    }

    /// Merge another fragment into this one (used by the K8s identity overlay
    /// layer to append on top of cloud emitters).
    pub fn extend(&mut self, other: TfFragment) {
        self.resource_blocks.extend(other.resource_blocks);
        self.data_blocks.extend(other.data_blocks);
        self.locals.extend(other.locals);
        self.shared_addresses.extend(other.shared_addresses);
        for (address, input_ids) in other.gated_contributions {
            self.declare_contribution(address, &input_ids);
        }
    }
}

/// (`type`, `label`) address of a `resource` block, `None` for anything that
/// is not a two-label resource block.
fn block_address(block: &Block) -> Option<(String, String)> {
    if block.identifier.as_str() != "resource" {
        return None;
    }
    let provider_type = block.labels.first()?.as_str().to_string();
    let label = block.labels.get(1)?.as_str().to_string();
    Some((provider_type, label))
}

/// Generator-side trait \u2014 emit the raw `resource`/`data` blocks for one stack
/// resource plus an `hcl::Expression` that resolves to its typed `ImportData`
/// at apply time.
pub trait TfEmitter: Send + Sync {
    /// Emit the raw Terraform blocks that back this stack resource. The
    /// generator merges the fragment into the module body.
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment>;

    /// Emit with access to the full registry. Resource emitters that need
    /// linked-resource binding references can override this while older
    /// emitters keep implementing the simpler method.
    fn emit_with_registry(
        &self,
        ctx: &EmitContext<'_>,
        _registry: &TfRegistry,
    ) -> Result<TfFragment> {
        self.emit(ctx)
    }

    /// Apply-time expression that resolves to this resource's typed
    /// `ImportData`. Embedded in the module's `deployment_resources` local + a
    /// per-resource output. Typically an HCL object built from `aws_x.y.z`
    /// references.
    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression>;

    /// Apply-time expression that resolves to this resource's runtime binding
    /// payload. This is intentionally separate from [`Self::emit_import_ref`]:
    /// import data feeds the manager, while binding data feeds user code.
    ///
    /// A gated resource's own references need the same `[0]` indexing they get
    /// in [`Self::emit_import_ref`]. A gated resource still cannot reach here:
    /// compute is live-only, so a gated Worker never renders into setup
    /// Terraform, and `ResourceEnabledValidCheck` rejects an ungated dependent
    /// of a gated resource outright. This is only reachable if compute gains a
    /// frozen lifecycle or that rule relaxes.
    fn emit_binding_ref(&self, _ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        Ok(None)
    }
}
