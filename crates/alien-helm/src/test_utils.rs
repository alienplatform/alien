use indexmap::IndexMap;
use std::{ffi::OsStr, fs, path::Path, process::Command};

/// Files written for linters that operate on a directory.
pub type LinterFiles = IndexMap<String, String>;

/// Result of a linter command or linter command sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinterRun {
    pub tool: String,
    pub command: String,
    pub status: LinterStatus,
    pub stdout: String,
    pub stderr: String,
}

/// High-level linter status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinterStatus {
    Passed,
    Failed(Option<i32>),
    Skipped(String),
}

impl LinterRun {
    pub fn skipped(tool: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            tool: tool.into(),
            command: String::new(),
            status: LinterStatus::Skipped(reason.into()),
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    pub fn is_ok(&self) -> bool {
        matches!(self.status, LinterStatus::Passed)
    }

    pub fn assert_ok(&self, context: impl AsRef<str>) {
        match &self.status {
            LinterStatus::Passed => {}
            LinterStatus::Skipped(reason) => {
                panic!(
                    "{} was skipped for {}\nreason: {}",
                    self.tool,
                    context.as_ref(),
                    reason
                );
            }
            LinterStatus::Failed(code) => {
                panic!(
                    "{} failed for {}\ncommand: {}\nexit: {:?}\nstdout:\n{}\nstderr:\n{}",
                    self.tool,
                    context.as_ref(),
                    self.command,
                    code,
                    self.stdout,
                    self.stderr
                );
            }
        }
    }
}

/// Run `helm lint` against generated chart files.
pub fn helm_lint(files: &LinterFiles) -> LinterRun {
    run_when_enabled("helm lint", || {
        let dir = write_files_to_temp_dir(files)?;
        run_command("helm", [OsStr::new("lint"), dir.path().as_os_str()], None)
    })
}

/// Render a chart with `helm template`, then validate with kubeconform.
pub fn helm_template_and_validate(files: &LinterFiles, values_yaml: Option<&str>) -> LinterRun {
    helm_template_and_validate_for_release(files, values_yaml, "test-release")
}

/// Render a chart for an explicit release name, then validate with kubeconform.
pub fn helm_template_and_validate_for_release(
    files: &LinterFiles,
    values_yaml: Option<&str>,
    release_name: &str,
) -> LinterRun {
    run_when_enabled("helm template", || {
        let dir = write_files_to_temp_dir(files)?;
        let values_path = if let Some(values) = values_yaml {
            let path = dir.path().join("test-values.yaml");
            write_file(&path, values)?;
            Some(path)
        } else {
            None
        };

        let mut args = vec![
            OsStr::new("template").to_os_string(),
            OsStr::new(release_name).to_os_string(),
            dir.path().as_os_str().to_os_string(),
        ];
        if let Some(path) = &values_path {
            args.push(OsStr::new("-f").to_os_string());
            args.push(path.as_os_str().to_os_string());
        }

        let rendered = run_command("helm", args.iter().map(|arg| arg.as_os_str()), None)?;
        let rendered = helm_template_with_debug_on_failure(rendered, &args)?;
        if !matches!(rendered.status, LinterStatus::Passed) {
            return Ok(rendered);
        }

        let rendered_path = dir.path().join("rendered.yaml");
        write_file(&rendered_path, &rendered.stdout)?;
        run_command(
            "kubeconform",
            [
                OsStr::new("-strict"),
                OsStr::new("-summary"),
                // kubeconform intentionally has no built-in schema for the CRD
                // object itself. Helm/Kind lifecycle coverage submits it to the
                // Kubernetes API; keep validating every namespaced resource here.
                OsStr::new("-skip"),
                OsStr::new("CustomResourceDefinition"),
                OsStr::new("-kubernetes-version"),
                OsStr::new("1.28.0"),
                rendered_path.as_os_str(),
            ],
            None,
        )
    })
}

/// Render a chart with `helm template` and return the rendered manifest.
pub fn helm_template(files: &LinterFiles, values_yaml: Option<&str>) -> LinterRun {
    helm_template_for_release(files, values_yaml, "test-release")
}

/// Render a chart with `helm template` for an explicit release name.
pub fn helm_template_for_release(
    files: &LinterFiles,
    values_yaml: Option<&str>,
    release_name: &str,
) -> LinterRun {
    run_when_enabled("helm template", || {
        let dir = write_files_to_temp_dir(files)?;
        let values_path = if let Some(values) = values_yaml {
            let path = dir.path().join("test-values.yaml");
            write_file(&path, values)?;
            Some(path)
        } else {
            None
        };

        let mut args = vec![
            OsStr::new("template").to_os_string(),
            OsStr::new(release_name).to_os_string(),
            dir.path().as_os_str().to_os_string(),
        ];
        if let Some(path) = &values_path {
            args.push(OsStr::new("-f").to_os_string());
            args.push(path.as_os_str().to_os_string());
        }

        let rendered = run_command("helm", args.iter().map(|arg| arg.as_os_str()), None)?;
        helm_template_with_debug_on_failure(rendered, &args)
    })
}

/// Render a chart's NOTES.txt for a release in `namespace`. `helm template`
/// skips NOTES.txt and `helm install --dry-run` needs a cluster, so the notes
/// are rendered through a ConfigMap that includes them. Returns the notes text
/// in `stdout` on success.
pub fn helm_render_notes(
    files: &LinterFiles,
    values_yaml: &str,
    release_name: &str,
    namespace: &str,
) -> LinterRun {
    let Some(notes) = files.get("templates/NOTES.txt") else {
        return LinterRun {
            tool: "helm template".to_string(),
            command: "helm template".to_string(),
            status: LinterStatus::Failed(None),
            stdout: String::new(),
            stderr: "the chart has no templates/NOTES.txt".to_string(),
        };
    };
    let mut files = files.clone();
    files.insert(
        "templates/_notes_render.tpl".to_string(),
        format!("{{{{- define \"test.notesRender\" -}}}}\n{notes}\n{{{{- end -}}}}\n"),
    );
    files.insert(
        "templates/notes-render.yaml".to_string(),
        "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: notes-render\ndata:\n  notes: |\n{{ include \"test.notesRender\" . | indent 4 }}\n".to_string(),
    );
    run_when_enabled("helm template", || {
        let dir = write_files_to_temp_dir(&files)?;
        let values_path = dir.path().join("test-values.yaml");
        write_file(&values_path, values_yaml)?;
        let rendered = run_command(
            "helm",
            [
                OsStr::new("template"),
                OsStr::new(release_name),
                dir.path().as_os_str(),
                OsStr::new("--namespace"),
                OsStr::new(namespace),
                OsStr::new("--show-only"),
                OsStr::new("templates/notes-render.yaml"),
                OsStr::new("-f"),
                values_path.as_os_str(),
            ],
            None,
        )?;
        if !rendered.is_ok() {
            return Ok(rendered);
        }
        let manifest: serde_yaml::Value = serde_yaml::from_str(&rendered.stdout)
            .map_err(|error| format!("rendered notes ConfigMap is not YAML: {error}"))?;
        let notes = manifest["data"]["notes"]
            .as_str()
            .ok_or("rendered notes ConfigMap has no data.notes")?
            .to_string();
        Ok(LinterRun {
            stdout: notes,
            ..rendered
        })
    })
}

fn helm_template_with_debug_on_failure(
    rendered: LinterRun,
    args: &[std::ffi::OsString],
) -> Result<LinterRun, String> {
    if matches!(rendered.status, LinterStatus::Passed) {
        return Ok(rendered);
    }

    let mut debug_args = Vec::with_capacity(args.len() + 1);
    debug_args.push(OsStr::new("--debug").to_os_string());
    debug_args.extend(args.iter().cloned());
    let debug = run_command("helm", debug_args.iter().map(|arg| arg.as_os_str()), None)?;
    Ok(LinterRun {
        tool: rendered.tool,
        command: rendered.command,
        status: rendered.status,
        stdout: format!(
            "{}\n\n--- helm template --debug stdout ---\n{}",
            rendered.stdout, debug.stdout
        ),
        stderr: format!(
            "{}\n\n--- helm template --debug stderr ---\n{}",
            rendered.stderr, debug.stderr
        ),
    })
}

fn run_when_enabled<F>(tool: &str, run: F) -> LinterRun
where
    F: FnOnce() -> Result<LinterRun, String>,
{
    if std::env::var("SKIP_LINTERS").as_deref() == Ok("1") {
        return LinterRun::skipped(tool, "SKIP_LINTERS=1");
    }

    match run() {
        Ok(result) => result,
        Err(error) => LinterRun {
            tool: tool.to_string(),
            command: tool.to_string(),
            status: LinterStatus::Failed(None),
            stdout: String::new(),
            stderr: error,
        },
    }
}

fn write_files_to_temp_dir(files: &LinterFiles) -> Result<tempfile::TempDir, String> {
    let dir = tempfile::tempdir().map_err(|error| error.to_string())?;
    for (path, contents) in files {
        let full_path = dir.path().join(path);
        write_file(&full_path, contents)?;
    }
    Ok(dir)
}

fn write_file(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(path, contents).map_err(|error| error.to_string())
}

fn run_command<I, S>(
    program: &str,
    args: I,
    current_dir: Option<&Path>,
) -> Result<LinterRun, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args: Vec<String> = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string_lossy().to_string())
        .collect();
    let command = std::iter::once(program.to_string())
        .chain(args.iter().cloned())
        .collect::<Vec<_>>()
        .join(" ");

    let mut process = Command::new(program);
    process.args(&args);
    if let Some(current_dir) = current_dir {
        process.current_dir(current_dir);
    }

    let output = process.output().map_err(|error| error.to_string())?;
    let status = if output.status.success() {
        LinterStatus::Passed
    } else {
        LinterStatus::Failed(output.status.code())
    };

    Ok(LinterRun {
        tool: program.to_string(),
        command,
        status,
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}
