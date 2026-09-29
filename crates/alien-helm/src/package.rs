//! Turning a generated chart into an installable package: default values a
//! distributor bakes in (management endpoint, Operator image, log
//! collection, version) and the `.tgz` archive Helm installs.

use std::io::Write;

use alien_core::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use flate2::{write::GzEncoder, Compression};

use crate::HelmChart;

/// Defaults a distributor bakes into a chart's `values.yaml` and `Chart.yaml`.
///
/// Installers can still override every value; these only change what an
/// installation gets without extra flags.
#[derive(Debug, Clone, Default)]
pub struct PackageDefaults<'a> {
    /// Chart version (semver) and app version.
    pub version: Option<&'a str>,
    /// Management endpoint the Operator connects to (`management.defaultUrl`).
    pub management_url: Option<&'a str>,
    /// Operator image (`runtime.image.repository` / `runtime.image.tag`).
    pub operator_image: Option<OperatorImageDefault<'a>>,
    /// Pod log collection (`logCollector.enabled` / `logCollector.mode`).
    pub log_collector: Option<LogCollectorDefault>,
}

/// Operator image reference split the way the chart values expect it.
#[derive(Debug, Clone, Copy)]
pub struct OperatorImageDefault<'a> {
    /// Repository, e.g. `manager.example.com/alienplatform/alien-operator`.
    pub repository: &'a str,
    /// Tag, e.g. `1.4.0`.
    pub tag: &'a str,
}

/// How the Operator collects pod logs by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogCollectorDefault {
    /// The Operator reads selected pod logs through the namespaced API.
    PodApi,
    /// A Fluent Bit DaemonSet reads pod logs on each node.
    NodeAgent,
}

/// Apply `defaults` to a generated chart.
///
/// Edits are exact replacements of the generator's own default lines, so the
/// comments in `values.yaml` survive. A generator change that moves those
/// lines fails loudly here instead of producing a chart with stale defaults.
pub fn apply_package_defaults(
    mut chart: HelmChart,
    defaults: &PackageDefaults<'_>,
) -> Result<HelmChart> {
    if let Some(version) = defaults.version {
        let chart_yaml = file_mut(&mut chart, "Chart.yaml")?;
        replace_once(
            chart_yaml,
            "version: 0.1.0\n",
            &format!("version: {version}\n"),
        )?;
        replace_once(
            chart_yaml,
            "appVersion: \"0.1.0\"\n",
            &format!("appVersion: {}\n", quote(version)?),
        )?;
    }

    let values = file_mut(&mut chart, "values.yaml")?;
    if let Some(url) = defaults.management_url {
        replace_once(
            values,
            "  defaultUrl: \"\"\n",
            &format!("  defaultUrl: {}\n", quote(url)?),
        )?;
    }
    if let Some(image) = defaults.operator_image {
        replace_once(
            values,
            "    repository: registry.example.com/deployment/operator\n    tag: latest\n",
            &format!(
                "    repository: {}\n    tag: {}\n",
                quote(image.repository)?,
                quote(image.tag)?
            ),
        )?;
    }
    if let Some(collector) = defaults.log_collector {
        replace_once(
            values,
            "logCollector:\n  enabled: false\n",
            "logCollector:\n  enabled: true\n",
        )?;
        let mode = match collector {
            LogCollectorDefault::PodApi => "podApi",
            LogCollectorDefault::NodeAgent => "nodeAgent",
        };
        replace_once(values, "  mode: nodeAgent\n", &format!("  mode: {mode}\n"))?;
        let readme = file_mut(&mut chart, "README.md")?;
        replace_once(
            readme,
            "Log collection is off by default. Enable it and choose one mode:",
            &format!("Log collection is on by default in `{mode}` mode. To change it, set:"),
        )?;
    }
    Ok(chart)
}

/// Package a chart as the gzipped tarball Helm installs.
///
/// Entries are sorted with fixed metadata, so the same chart always produces
/// the same bytes (and digest).
pub fn package_chart(chart: &HelmChart) -> Result<Vec<u8>> {
    let failed = |message: &str| ErrorData::GenericError {
        message: format!("Failed to package chart '{}': {message}", chart.name),
    };
    let mut files: Vec<_> = chart.files.iter().collect();
    files.sort_by(|a, b| a.0.cmp(b.0));

    let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    for (path, contents) in files {
        let mut header = tar::Header::new_ustar();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        builder
            .append_data(
                &mut header,
                format!("{}/{}", chart.name, path),
                contents.as_bytes(),
            )
            .into_alien_error()
            .context(failed(&format!("adding {path}")))?;
    }
    let mut encoder = builder
        .into_inner()
        .into_alien_error()
        .context(failed("finishing the archive"))?;
    encoder
        .flush()
        .into_alien_error()
        .context(failed("flushing"))?;
    encoder
        .finish()
        .into_alien_error()
        .context(failed("compressing"))
}

fn file_mut<'a>(chart: &'a mut HelmChart, name: &str) -> Result<&'a mut String> {
    let chart_name = chart.name.clone();
    chart.files.get_mut(name).ok_or_else(|| {
        AlienError::new(ErrorData::GenericError {
            message: format!("Generated chart '{chart_name}' has no {name}"),
        })
    })
}

fn replace_once(contents: &mut String, expected: &str, replacement: &str) -> Result<()> {
    if contents.matches(expected).count() != 1 {
        return Err(AlienError::new(ErrorData::GenericError {
            message: format!(
                "Generated chart defaults changed unexpectedly: expected exactly one {expected:?}"
            ),
        }));
    }
    *contents = contents.replacen(expected, replacement, 1);
    Ok(())
}

fn quote(value: &str) -> Result<String> {
    serde_json::to_string(value)
        .into_alien_error()
        .context(ErrorData::GenericError {
            message: "Failed to encode chart default".to_string(),
        })
}
