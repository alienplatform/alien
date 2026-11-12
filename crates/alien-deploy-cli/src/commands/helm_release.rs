//! The check that runs while the deploy CLI applies a chart with `helm upgrade --install`: a
//! chart whose stack's extensions have Helm modifiers carries their subcharts, and an object a
//! subchart renders may not share a kind and name with another object of the release.
//!
//! The check runs inside the install itself, as a Helm post-renderer plugin: it reads the exact
//! release Helm then applies, hooks included, after the release's state, `lookup`, the
//! cluster's capabilities and any random value resolved. The plugin is this executable's
//! hidden `check-helm-release` command, from a plugin directory made for the install. Helm
//! keeps a plugin's stderr to itself, so the command writes its refusal to a file the install
//! reads back.

use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use std::ffi::OsString;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use tokio::process::Command;

/// The name of the post-renderer plugin the install runs.
const PLUGIN: &str = "alien-release-check";

/// The hidden command Helm runs as the post-renderer.
#[derive(clap::Args, Debug)]
pub struct CheckReleaseArgs {
    /// An extension whose subchart the chart carries
    #[arg(long = "extension")]
    extensions: Vec<String>,
    /// Where to write the reason when the release is refused
    #[arg(long)]
    refusal: PathBuf,
}

/// Reads the release Helm hands a post-renderer from stdin, and writes it back unchanged to
/// stdout, unless an object repeats a kind and name; then it writes the reason to `--refusal`
/// and fails, which stops the install.
pub fn check_release_command(args: &CheckReleaseArgs) -> Result<()> {
    check_release(
        &args.extensions,
        &args.refusal,
        std::io::stdin().lock(),
        std::io::stdout().lock(),
    )
}

/// [`check_release_command`] on `input` and `output`.
fn check_release(
    extensions: &[String],
    refusal: &Path,
    mut input: impl Read,
    mut output: impl Write,
) -> Result<()> {
    let failed = |what: &str| ErrorData::ConfigurationError {
        message: format!("The release check could not {what}"),
    };
    let mut release = Vec::new();
    input
        .read_to_end(&mut release)
        .into_alien_error()
        .context(failed("read the release"))?;
    let checked = match std::str::from_utf8(&release) {
        Ok(manifest) => check_manifest(manifest, extensions),
        Err(_) => Err(
            "The Helm release can't be applied: Helm rendered it as text that isn't UTF-8"
                .to_string(),
        ),
    };
    if let Err(reason) = checked {
        std::fs::write(refusal, &reason)
            .into_alien_error()
            .context(failed("write its refusal"))?;
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: reason,
        }));
    }
    output
        .write_all(&release)
        .and_then(|()| output.flush())
        .into_alien_error()
        .context(failed("write the release"))
}

/// Refuses a post-rendered release where an object repeats a kind and name.
pub(crate) fn check_manifest(
    manifest: &str,
    extensions: &[String],
) -> std::result::Result<(), String> {
    let names: Vec<&str> = extensions.iter().map(String::as_str).collect();
    alien_helm::post_rendered_objects(manifest)
        .and_then(|objects| alien_helm::refuse_repeated_objects(&objects, &names))
        .map_err(|error| format!("The Helm release can't be applied: {}", error.message))
}

/// The extensions whose subcharts the chart at `chart` (a directory or a packaged `.tgz`)
/// carries: those of its stack (`files/stack.json`) with a `charts/<extension>/Chart.yaml`,
/// which the modifier pass writes only for an extension with Helm modifiers.
pub(crate) fn extension_subcharts(chart: &Path) -> Result<Vec<String>> {
    let unreadable = |path: PathBuf| {
        move |error: std::io::Error| {
            AlienError::new(ErrorData::ConfigurationError {
                message: format!("The Helm chart's {} can't be read: {error}", path.display()),
            })
        }
    };
    let (stack, subcharts) = if chart.is_dir() {
        let stack = match std::fs::read(chart.join("files/stack.json")) {
            Ok(stack) => stack,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(unreadable(chart.join("files/stack.json"))(error)),
        };
        let mut subcharts = Vec::new();
        match std::fs::read_dir(chart.join("charts")) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry.map_err(unreadable(chart.join("charts")))?;
                    if entry.path().join("Chart.yaml").is_file() {
                        subcharts.push(entry.file_name().to_string_lossy().into_owned());
                    }
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(unreadable(chart.join("charts"))(error)),
        }
        (stack, subcharts)
    } else {
        match packaged_chart(chart)? {
            (Some(stack), subcharts) => (stack, subcharts),
            (None, _) => return Ok(Vec::new()),
        }
    };
    let stack: alien_core::Stack = serde_json::from_slice(&stack).into_alien_error().context(
        ErrorData::ConfigurationError {
            message: "The Helm chart's files/stack.json does not read".to_string(),
        },
    )?;
    Ok(stack
        .extensions
        .into_iter()
        .map(|extension| extension.name)
        .filter(|name| subcharts.contains(name))
        .collect())
}

/// A packaged chart's `files/stack.json` and the names of the subcharts it has a `Chart.yaml`
/// for, read from the archive.
fn packaged_chart(chart: &Path) -> Result<(Option<Vec<u8>>, Vec<String>)> {
    let unreadable = || ErrorData::ConfigurationError {
        message: format!(
            "The Helm chart {} is not a readable archive",
            chart.display()
        ),
    };
    let file = std::fs::File::open(chart)
        .into_alien_error()
        .context(unreadable())?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let (mut stack, mut subcharts) = (None, Vec::new());
    for entry in archive.entries().into_alien_error().context(unreadable())? {
        let mut entry = entry.into_alien_error().context(unreadable())?;
        let path = entry
            .path()
            .into_alien_error()
            .context(unreadable())?
            .into_owned();
        // `<chart>/files/stack.json` and `<chart>/charts/<subchart>/Chart.yaml`.
        let parts: Vec<String> = path
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();
        match parts.as_slice() {
            [_, files, name] if files == "files" && name == "stack.json" => {
                let mut contents = Vec::new();
                std::io::Read::read_to_end(&mut entry, &mut contents)
                    .into_alien_error()
                    .context(unreadable())?;
                stack = Some(contents);
            }
            [_, charts, subchart, name] if charts == "charts" && name == "Chart.yaml" => {
                subcharts.push(subchart.clone());
            }
            _ => {}
        }
    }
    Ok((stack, subcharts))
}

/// The executable Helm runs as the post-renderer: this one, or in tests the stand-in a test
/// sets.
fn checker() -> Result<PathBuf> {
    #[cfg(test)]
    if let Some(checker) = tests::CHECKER.lock().unwrap().clone() {
        return Ok(checker);
    }
    std::env::current_exe()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "The deploy CLI can't find its own executable for Helm's release check"
                .to_string(),
        })
}

/// The release check an install runs: the plugin directory Helm loads it from, and the file it
/// writes its refusal to.
pub(crate) struct ReleaseCheck {
    directory: tempfile::TempDir,
}

impl ReleaseCheck {
    fn refusal_path(&self) -> PathBuf {
        self.directory.path().join("refusal")
    }

    /// `result` of the install, with the release check's refusal in place of Helm's error when
    /// the check stopped it.
    pub(crate) fn explain<T>(&self, result: Result<T>) -> Result<T> {
        match (result, std::fs::read_to_string(self.refusal_path())) {
            (Err(_), Ok(refusal)) if !refusal.is_empty() => {
                Err(AlienError::new(ErrorData::ConfigurationError {
                    message: refusal,
                }))
            }
            (result, _) => result,
        }
    }
}

/// Adds the release check to `install`, a `helm upgrade --install` of the chart at `chart`, when
/// the chart carries an extension's subchart: `--post-renderer` with a plugin directory ahead
/// of the user's own on `HELM_PLUGINS`. A chart without one gets no change to its command.
pub(crate) async fn add_release_check(
    install: &mut Command,
    chart: &Path,
) -> Result<Option<ReleaseCheck>> {
    let extensions = extension_subcharts(chart)?;
    if extensions.is_empty() {
        return Ok(None);
    }
    let failed = |what: &str| ErrorData::ConfigurationError {
        message: format!("Failed to set up Helm's release check: {what}"),
    };
    let check = ReleaseCheck {
        directory: tempfile::tempdir()
            .into_alien_error()
            .context(failed("a temporary directory"))?,
    };
    let mut args = vec![
        "check-helm-release".to_string(),
        "--refusal".to_string(),
        check.refusal_path().display().to_string(),
    ];
    for extension in &extensions {
        args.extend(["--extension".to_string(), extension.clone()]);
    }
    let plugin = serde_json::json!({
        "apiVersion": "v1",
        "type": "postrenderer/v1",
        "name": PLUGIN,
        "version": "0.1.0",
        "runtime": "subprocess",
        "runtimeConfig": {
            "platformCommand": [{ "command": checker()?.display().to_string(), "args": args }]
        }
    });
    let plugin_directory = check.directory.path().join("plugins").join(PLUGIN);
    std::fs::create_dir_all(&plugin_directory)
        .into_alien_error()
        .context(failed("the plugin directory"))?;
    std::fs::write(
        plugin_directory.join("plugin.yaml"),
        serde_yaml::to_string(&plugin)
            .into_alien_error()
            .context(failed("the plugin manifest"))?,
    )
    .into_alien_error()
    .context(failed("the plugin manifest"))?;

    // The user's own plugins stay where Helm finds them.
    let installed = Command::new(install.as_std().get_program())
        .args(["env", "HELM_PLUGINS"])
        .output()
        .await
        .into_alien_error()
        .context(failed("helm env HELM_PLUGINS"))?;
    if !installed.status.success() {
        return Err(AlienError::new(failed(&format!(
            "helm env HELM_PLUGINS, which finds the user's own plugins, failed: {}",
            String::from_utf8_lossy(&installed.stderr).trim()
        ))));
    }
    let plugins = helm_plugins(
        &check.directory.path().join("plugins"),
        String::from_utf8_lossy(&installed.stdout).trim(),
    )
    .map_err(|error| AlienError::new(failed(&error)))?;
    install
        .env("HELM_PLUGINS", plugins)
        .args(["--post-renderer", PLUGIN]);
    Ok(Some(check))
}

/// `HELM_PLUGINS` with `ours` ahead of `installed`, the user's own plugin directories as
/// `helm env` lists them, in the platform's path-list form.
fn helm_plugins(ours: &Path, installed: &str) -> std::result::Result<OsString, String> {
    let installed = std::env::split_paths(installed).filter(|path| !path.as_os_str().is_empty());
    std::env::join_paths(std::iter::once(ours.to_path_buf()).chain(installed)).map_err(|error| {
        format!(
            "the plugin directory {} can't be listed on HELM_PLUGINS: {error}",
            ours.display()
        )
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The executable a test has Helm run as the post-renderer in place of this one.
    pub(crate) static CHECKER: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

    /// Held by the tests that set [`CHECKER`] or read the plugin it writes, so none sees
    /// another's stand-in.
    pub(crate) static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// Puts `checker` in place of this executable until dropped, even when the test fails.
    pub(crate) struct StandIn;

    impl StandIn {
        pub(crate) fn new(checker: PathBuf) -> Self {
            *CHECKER.lock().unwrap() = Some(checker);
            Self
        }
    }

    impl Drop for StandIn {
        fn drop(&mut self) {
            *CHECKER.lock().unwrap() = None;
        }
    }

    fn arguments(command: &Command) -> Vec<OsString> {
        command
            .as_std()
            .get_args()
            .map(|argument| argument.to_os_string())
            .collect()
    }

    fn install() -> Command {
        let mut install = Command::new("helm");
        install.args(["upgrade", "--install", "rel", "/chart", "--namespace", "ns"]);
        install
    }

    /// A chart directory whose stack lists `acme`, with its subchart when `subchart`.
    fn chart(subchart: bool) -> tempfile::TempDir {
        let chart = tempfile::tempdir().unwrap();
        let mut stack = alien_core::Stack::new("app".to_string()).build();
        stack.extensions = vec![serde_json::from_value(serde_json::json!({
            "name": "acme", "version": "1.0.0", "digest": "sha256:0", "resourceTypes": {}
        }))
        .unwrap()];
        std::fs::create_dir_all(chart.path().join("files")).unwrap();
        std::fs::write(
            chart.path().join("files/stack.json"),
            serde_json::to_string(&stack).unwrap(),
        )
        .unwrap();
        if subchart {
            std::fs::create_dir_all(chart.path().join("charts/acme")).unwrap();
            std::fs::write(chart.path().join("charts/acme/Chart.yaml"), "name: acme\n").unwrap();
        }
        chart
    }

    #[tokio::test]
    async fn a_chart_without_an_extension_s_subchart_keeps_its_command() {
        let chart = chart(false);
        let mut install = install();
        let before = arguments(&install);
        assert!(add_release_check(&mut install, chart.path())
            .await
            .unwrap()
            .is_none());
        assert_eq!(arguments(&install), before);
        assert_eq!(install.as_std().get_envs().count(), 0);
    }

    #[tokio::test]
    async fn the_install_runs_the_check_as_a_post_renderer_ahead_of_the_user_s_plugins() {
        let _serial = SERIAL.lock().await;
        let chart = chart(true);
        let mut install = install();
        let check = add_release_check(&mut install, chart.path())
            .await
            .unwrap()
            .unwrap();
        let args = arguments(&install);
        assert_eq!(
            args[args.len() - 2..],
            [OsString::from("--post-renderer"), OsString::from(PLUGIN)]
        );
        let plugins = check.directory.path().join("plugins");
        let installed = std::process::Command::new("helm")
            .args(["env", "HELM_PLUGINS"])
            .output()
            .unwrap();
        let installed = String::from_utf8(installed.stdout).unwrap();
        let (_, helm_plugins) = install
            .as_std()
            .get_envs()
            .find(|(key, _)| *key == "HELM_PLUGINS")
            .unwrap();
        let mut expected = vec![plugins.clone()];
        expected.extend(std::env::split_paths(installed.trim()));
        assert_eq!(
            std::env::split_paths(helm_plugins.unwrap()).collect::<Vec<_>>(),
            expected
        );
        let manifest: serde_yaml::Value = serde_yaml::from_str(
            &std::fs::read_to_string(plugins.join(PLUGIN).join("plugin.yaml")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["type"], "postrenderer/v1");
        let command = &manifest["runtimeConfig"]["platformCommand"][0];
        assert_eq!(
            command["command"].as_str().unwrap(),
            checker().unwrap().display().to_string()
        );
        let refusal = check.refusal_path().display().to_string();
        assert_eq!(
            command["args"],
            serde_yaml::to_value([
                "check-helm-release",
                "--refusal",
                refusal.as_str(),
                "--extension",
                "acme"
            ])
            .unwrap()
        );
    }

    #[test]
    fn a_refusal_replaces_helm_s_error() {
        let check = ReleaseCheck {
            directory: tempfile::tempdir().unwrap(),
        };
        let helm_error = || -> Result<()> {
            Err(AlienError::new(ErrorData::ConfigurationError {
                message: "plugin exited with error".to_string(),
            }))
        };
        assert_eq!(
            check.explain(helm_error()).unwrap_err().message,
            "Configuration error: plugin exited with error"
        );
        std::fs::write(
            check.refusal_path(),
            "extension 'acme' renders Secret ns/rel",
        )
        .unwrap();
        assert!(check
            .explain(helm_error())
            .unwrap_err()
            .message
            .contains("extension 'acme' renders Secret ns/rel"));
        assert!(check.explain(Ok(())).is_ok());
    }

    /// An error reading the chart's stack or subcharts fails the install rather than
    /// skipping the check.
    #[cfg(unix)]
    #[test]
    fn an_unreadable_stack_fails_closed() {
        use std::os::unix::fs::PermissionsExt;
        let chart = chart(true);
        let stack = chart.path().join("files/stack.json");
        std::fs::set_permissions(&stack, std::fs::Permissions::from_mode(0o000)).unwrap();
        assert!(extension_subcharts(chart.path()).is_err());
        std::fs::set_permissions(&stack, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::write(&stack, "not json").unwrap();
        assert!(extension_subcharts(chart.path()).is_err());
    }

    #[test]
    fn a_post_rendered_release_that_repeats_an_object_is_refused() {
        let manifest = "apiVersion: v1\nkind: Secret\nmetadata:\n  name: rel\n  annotations:\n    postrenderer.helm.sh/postrender-filename: 'app/templates/secret.yaml'\n---\napiVersion: v1\nkind: Secret\nmetadata:\n  name: rel\n  annotations:\n    postrenderer.helm.sh/postrender-filename: 'app/charts/acme/templates/zz.yaml'\n";
        assert_eq!(
            check_manifest(manifest, &["acme".to_string()]),
            Err("The Helm release can't be applied: extension 'acme' renders Secret rel (app/templates/secret.yaml and app/charts/acme/templates/zz.yaml), a kind and name the release already has; objects of one kind and name collide whatever namespace each names".to_string())
        );
    }

    /// Helm applies what the post-renderer writes, so a release the check passes goes back
    /// exactly as it came, and a refused one goes back not at all, with the reason in the
    /// refusal file.
    #[test]
    fn a_release_the_check_passes_reaches_helm_byte_for_byte() {
        let directory = tempfile::tempdir().unwrap();
        let refusal = directory.path().join("refusal");
        let extensions = ["acme".to_string()];
        let release = b"apiVersion: v1\r\nkind: Secret\r\nmetadata:\r\n  name: rel\r\n---\napiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: acme   \n  annotations: {}\n# no newline at the end";
        let mut output = Vec::new();
        check_release(&extensions, &refusal, &release[..], &mut output).unwrap();
        assert_eq!(output, release);
        assert!(!refusal.exists());

        let repeated = "apiVersion: v1\nkind: Secret\nmetadata:\n  name: rel\n---\napiVersion: v1\nkind: Secret\nmetadata:\n  name: rel\n  annotations:\n    postrenderer.helm.sh/postrender-filename: 'app/charts/acme/templates/zz.yaml'\n";
        let mut output = Vec::new();
        let error =
            check_release(&extensions, &refusal, repeated.as_bytes(), &mut output).unwrap_err();
        assert!(output.is_empty());
        let reason = std::fs::read_to_string(&refusal).unwrap();
        assert!(
            reason.starts_with(
                "The Helm release can't be applied: extension 'acme' renders Secret rel"
            ),
            "{reason}"
        );
        assert_eq!(error.message, format!("Configuration error: {reason}"));

        let mut output = Vec::new();
        assert!(check_release(&extensions, &refusal, &b"kind: \xff"[..], &mut output).is_err());
        assert!(output.is_empty());
    }

    /// A `helm` whose `helm env HELM_PLUGINS` runs `answer`, installing the chart in `install`.
    #[cfg(unix)]
    fn helm_answering(directory: &Path, answer: &str) -> Command {
        let helm = directory.join("helm");
        std::fs::write(&helm, format!("#!/bin/sh\n{answer}\n")).unwrap();
        std::fs::set_permissions(&helm, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        let mut install = Command::new(helm);
        install.args(["upgrade", "--install", "rel", "/chart", "--namespace", "ns"]);
        install
    }

    /// Each of the user's plugin directories stays on `HELM_PLUGINS`, behind the check's, in
    /// the platform's own path-list form.
    #[cfg(unix)]
    #[tokio::test]
    async fn every_plugin_directory_of_the_user_s_follows_the_check_s() {
        let directory = tempfile::tempdir().unwrap();
        let chart = chart(true);
        let mut install = helm_answering(directory.path(), "echo /a/plugins:/b/plugins");
        let check = add_release_check(&mut install, chart.path())
            .await
            .unwrap()
            .unwrap();
        let (_, plugins) = install
            .as_std()
            .get_envs()
            .find(|(key, _)| *key == "HELM_PLUGINS")
            .unwrap();
        assert_eq!(
            std::env::split_paths(plugins.unwrap()).collect::<Vec<_>>(),
            [
                check.directory.path().join("plugins"),
                PathBuf::from("/a/plugins"),
                PathBuf::from("/b/plugins"),
            ]
        );
        assert_eq!(
            helm_plugins(Path::new("/ours"), "").unwrap(),
            OsString::from("/ours")
        );
        assert!(helm_plugins(Path::new("/our:s"), "/a").is_err());
    }

    /// When Helm can't say where the user's plugins are, the install stops rather than run
    /// without them.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_failing_helm_env_fails_the_install() {
        let directory = tempfile::tempdir().unwrap();
        let chart = chart(true);
        let mut install = helm_answering(
            directory.path(),
            "echo 'Error: unreadable plugins' >&2; exit 1",
        );
        let error = add_release_check(&mut install, chart.path())
            .await
            .err()
            .expect("the install stops");
        assert!(
            error.message.contains("helm env HELM_PLUGINS")
                && error.message.contains("Error: unreadable plugins"),
            "{}",
            error.message
        );
    }

    /// An extension without Helm modifiers adds no subchart, so its chart gets no check.
    #[test]
    fn only_an_extension_with_a_subchart_is_checked() {
        let chart = tempfile::tempdir().unwrap();
        let mut stack = alien_core::Stack::new("app".to_string()).build();
        stack.extensions = ["acme", "tags"]
            .iter()
            .map(|name| {
                serde_json::from_value(serde_json::json!({
                    "name": name, "version": "1.0.0", "digest": "sha256:0", "resourceTypes": {}
                }))
                .unwrap()
            })
            .collect();
        std::fs::create_dir_all(chart.path().join("files")).unwrap();
        std::fs::write(
            chart.path().join("files/stack.json"),
            serde_json::to_string(&stack).unwrap(),
        )
        .unwrap();
        assert!(extension_subcharts(chart.path()).unwrap().is_empty());
        std::fs::create_dir_all(chart.path().join("charts/acme")).unwrap();
        std::fs::write(chart.path().join("charts/acme/Chart.yaml"), "name: acme\n").unwrap();
        assert_eq!(extension_subcharts(chart.path()).unwrap(), ["acme"]);
        // The same chart packaged, as `sync` installs it.
        let packaged = tempfile::tempdir().unwrap();
        let archive = packaged.path().join("app-0.1.0.tgz");
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            std::fs::File::create(&archive).unwrap(),
            flate2::Compression::default(),
        ));
        builder.append_dir_all("app", chart.path()).unwrap();
        builder.into_inner().unwrap().finish().unwrap();
        assert_eq!(extension_subcharts(&archive).unwrap(), ["acme"]);
        // A chart without a stack, such as one built before extensions, has none.
        assert!(extension_subcharts(&chart.path().join("charts/acme"))
            .unwrap()
            .is_empty());
    }
}
