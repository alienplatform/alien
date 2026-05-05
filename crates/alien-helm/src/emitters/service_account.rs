//! ServiceAccount emitter — feeds into the chart-level
//! `templates/serviceaccount.yaml`. The actual annotations (IRSA /
//! Workload Identity / Federated Identity) land via the chart-level
//! `examples/<target>.yaml` files the customer copies. This emitter
//! contributes nothing to the values tree (the SA is rendered from
//! `values.serviceAccounts`).

use crate::emitter::{HelmEmitter, HelmFragment};
use alien_core::{import::EmitContext, Result};

#[derive(Debug, Default)]
pub struct ServiceAccountEmitter;

impl HelmEmitter for ServiceAccountEmitter {
    fn emit(&self, _ctx: &EmitContext<'_>) -> Result<HelmFragment> {
        Ok(HelmFragment::empty())
    }
}
