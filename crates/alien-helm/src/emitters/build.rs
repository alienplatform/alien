//! Build emitter — Builds run via the platform-side build controller, so
//! the chart only needs to know about the permission profile (used to
//! generate a ServiceAccount). No infrastructure binding required.

use crate::emitter::{HelmEmitter, HelmFragment};
use alien_core::{import::EmitContext, Result};

#[derive(Debug, Default)]
pub struct BuildEmitter;

impl HelmEmitter for BuildEmitter {
    fn emit(&self, _ctx: &EmitContext<'_>) -> Result<HelmFragment> {
        Ok(HelmFragment::empty())
    }
}
