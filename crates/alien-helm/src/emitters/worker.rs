//! Worker emitter — Workers become K8s Deployments + (when
//! `Ingress::Public`) a Service. The chart-level Deployment template
//! handles the actual K8s-native shape; this emitter contributes nothing
//! to `infrastructure.<id>` because Workers ARE the workload.

use crate::emitter::{HelmEmitter, HelmFragment};
use alien_core::{import::EmitContext, Result};

#[derive(Debug, Default)]
pub struct WorkerEmitter;

impl HelmEmitter for WorkerEmitter {
    fn emit(&self, _ctx: &EmitContext<'_>) -> Result<HelmFragment> {
        Ok(HelmFragment::empty())
    }
}
