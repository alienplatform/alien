//! The Helm chart modifier, which adds templates and changes none of Alien's.

use alien_core::hooks::{HelmModifierOutput, TemplateFiles};
use alien_core::Result;
use indexmap::IndexMap;
use std::collections::BTreeMap;

/// Adds files to the Helm chart Alien generates. Alien's own templates contain Go template
/// actions, so they can't be read and edited as YAML; a modifier can only add new ones.
pub trait HelmModifier {
    /// New templates, by file name, for the extension's own subchart, `charts/<extension>/`,
    /// whose values are its own. A name is lowercase letters, digits, `.`, `_` and `-`, and
    /// ends in `.yaml`, `.yml` or `.tpl`; Alien refuses any other name, a name the chart already
    /// has in any case, and a named template (`define`, `block`) outside the `<extension>.`
    /// prefix.
    fn templates(&self, chart: &HelmChart) -> Result<BTreeMap<String, String>>;
}

/// The Helm chart Alien generated, to read.
pub struct HelmChart {
    files: IndexMap<String, String>,
}

impl HelmChart {
    /// A chart from its files by path, as a test builds one.
    pub fn new(files: IndexMap<String, String>) -> Self {
        Self { files }
    }

    /// The chart's files by path, such as `Chart.yaml` and `templates/deployment.yaml`.
    pub fn files(&self) -> &IndexMap<String, String> {
        &self.files
    }
}

pub(crate) fn run(
    modifier: &dyn HelmModifier,
    input: &[u8],
) -> std::result::Result<Vec<u8>, alien_core::hooks::HookError> {
    let input: TemplateFiles = crate::read_input(input, "Helm chart")?;
    let chart = HelmChart::new(input.files);
    let templates = modifier
        .templates(&chart)
        .map_err(alien_core::hooks::HookError::from)?;
    crate::write_output(&HelmModifierOutput { templates })
}
