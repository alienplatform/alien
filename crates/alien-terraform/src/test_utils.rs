use indexmap::IndexMap;
use std::{
    ffi::{OsStr, OsString},
    fs,
    path::Path,
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

/// Run `terraform init -backend=false` and `terraform plan` with explicit
/// variables. This is useful for input validation rules that `validate` parses
/// but does not evaluate.
pub fn terraform_plan_with_vars(files: &LinterFiles, vars: &[(&str, &str)]) -> LinterRun {
    run_when_enabled("terraform plan", || {
        let dir = write_files_to_temp_dir(files)?;
        let init = run_command(
            "terraform",
            [OsStr::new("init"), OsStr::new("-backend=false")],
            Some(dir.path()),
        )?;
        if !matches!(init.status, LinterStatus::Passed) {
            return Ok(init);
        }

        let mut args = vec![
            OsString::from("plan"),
            OsString::from("-input=false"),
            OsString::from("-no-color"),
        ];
        for (name, value) in vars {
            args.push(OsString::from("-var"));
            args.push(OsString::from(format!("{name}={value}")));
        }

        run_command("terraform", args, Some(dir.path()))
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
