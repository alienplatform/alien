use crate::{ErrorData, Result};
use alien_error::{Context, ContextError, IntoAlienError, IntoAlienErrorDirect};
use indexmap::IndexMap;
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

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
    /// Create a skipped linter result.
    pub fn skipped(tool: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            tool: tool.into(),
            command: String::new(),
            status: LinterStatus::Skipped(reason.into()),
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    /// True when the linter passed or was explicitly skipped.
    pub fn is_ok(&self) -> bool {
        matches!(self.status, LinterStatus::Passed | LinterStatus::Skipped(_))
    }

    /// Assert the linter passed. Skips are accepted only when the `linters`
    /// feature is disabled or `SKIP_LINTERS=1` is set.
    pub fn assert_ok(&self, context: impl AsRef<str>) {
        match &self.status {
            LinterStatus::Passed => {}
            LinterStatus::Skipped(reason) => {
                eprintln!("skipped {} for {}: {}", self.tool, context.as_ref(), reason);
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

/// Run cfn-lint against a CloudFormation YAML template.
pub fn cfn_lint(template_yaml: &str) -> LinterRun {
    run_when_enabled("cfn-lint", || {
        let dir = tempfile::tempdir().map_err(|error| file_error(error, "<tempdir>", "create"))?;
        let path = dir.path().join("template.yaml");
        write_file(&path, template_yaml)?;
        run_command(
            "cfn-lint",
            [
                path.as_os_str(),
                OsStr::new("-i"),
                OsStr::new("W3005"),
                OsStr::new("-i"),
                OsStr::new("W1030"),
            ],
            None,
        )
    })
}

/// Run `terraform fmt -check -diff -recursive` against generated files.
pub fn terraform_fmt_check(files: &LinterFiles) -> LinterRun {
    run_when_enabled("terraform fmt", || {
        let dir = write_files_to_temp_dir(files)?;
        run_command(
            "terraform",
            [
                OsStr::new("fmt"),
                OsStr::new("-check"),
                OsStr::new("-diff"),
                OsStr::new("-recursive"),
            ],
            Some(dir.path()),
        )
    })
}

/// Run `terraform init -backend=false` and `terraform validate`.
pub fn terraform_validate(files: &LinterFiles) -> LinterRun {
    run_when_enabled("terraform validate", || {
        let dir = write_files_to_temp_dir(files)?;
        let init = run_command(
            "terraform",
            [OsStr::new("init"), OsStr::new("-backend=false")],
            Some(dir.path()),
        )?;
        if !matches!(init.status, LinterStatus::Passed) {
            return Ok(init);
        }

        run_command("terraform", [OsStr::new("validate")], Some(dir.path()))
    })
}

/// Run tflint against generated files.
pub fn tflint(files: &LinterFiles) -> LinterRun {
    run_when_enabled("tflint", || {
        let dir = write_files_to_temp_dir(files)?;
        run_command(
            "tflint",
            [OsStr::new("--chdir"), dir.path().as_os_str()],
            None,
        )
    })
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
            OsStr::new("test-release").to_os_string(),
            dir.path().as_os_str().to_os_string(),
        ];
        if let Some(path) = &values_path {
            args.push(OsStr::new("-f").to_os_string());
            args.push(path.as_os_str().to_os_string());
        }

        let rendered = run_command("helm", args.iter().map(|arg| arg.as_os_str()), None)?;
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
                OsStr::new("-kubernetes-version"),
                OsStr::new("1.28.0"),
                rendered_path.as_os_str(),
            ],
            None,
        )
    })
}

fn run_when_enabled<F>(tool: &str, run: F) -> LinterRun
where
    F: FnOnce() -> Result<LinterRun>,
{
    if std::env::var("SKIP_LINTERS").as_deref() == Ok("1") {
        return LinterRun::skipped(tool, "SKIP_LINTERS=1");
    }

    #[cfg(not(feature = "linters"))]
    {
        let _ = run;
        LinterRun::skipped(tool, "alien-test-kit linters feature is disabled")
    }

    #[cfg(feature = "linters")]
    {
        match run() {
            Ok(result) => result,
            Err(error) => LinterRun {
                tool: tool.to_string(),
                command: tool.to_string(),
                status: LinterStatus::Failed(None),
                stdout: String::new(),
                stderr: error.to_string(),
            },
        }
    }
}

fn write_files_to_temp_dir(files: &LinterFiles) -> Result<tempfile::TempDir> {
    let dir = tempfile::tempdir().map_err(|error| file_error(error, "<tempdir>", "create"))?;
    for (path, contents) in files {
        let full_path = dir.path().join(path);
        write_file(&full_path, contents)?;
    }
    Ok(dir)
}

fn write_file(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                path: parent.display().to_string(),
                operation: "create_dir_all".to_string(),
            })?;
    }

    fs::write(path, contents)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            path: path.display().to_string(),
            operation: "write".to_string(),
        })
}

fn file_error(
    error: std::io::Error,
    path: &str,
    operation: &str,
) -> alien_error::AlienError<ErrorData> {
    error
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            path: path.to_string(),
            operation: operation.to_string(),
        })
}

fn run_command<I, S>(program: &str, args: I, current_dir: Option<&Path>) -> Result<LinterRun>
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

    let output = process
        .output()
        .into_alien_error()
        .context(ErrorData::LinterCommandFailed {
            command: command.clone(),
        })?;
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

#[allow(dead_code)]
fn pathbuf(path: impl Into<PathBuf>) -> PathBuf {
    path.into()
}
