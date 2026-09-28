//! Kubernetes pools use the installation namespace and existing node capacity.
//! No node provisioning, additional RBAC, or installer configuration is needed.

use crate::emitter::{HelmEmitter, HelmFragment};
use alien_core::{import::EmitContext, Result};

#[derive(Debug, Default)]
pub struct ComputeClusterEmitter;

impl HelmEmitter for ComputeClusterEmitter {
    fn emit(&self, _ctx: &EmitContext<'_>) -> Result<HelmFragment> {
        // The operator verifies namespace access and applies pool placement to
        // static and dynamic workloads. This declaration owns no node fleet.
        Ok(HelmFragment::empty())
    }
}
