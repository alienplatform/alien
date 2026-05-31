use std::{ffi::OsStr, fs, path::Path, process::Command};

/// Result of a linter command.
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
        matches!(self.status, LinterStatus::Passed | LinterStatus::Skipped(_))
    }

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
        let dir = tempfile::tempdir().map_err(|error| error.to_string())?;
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
        )
        .map(suppress_known_cfn_lint_false_positives)
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

fn write_file(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(path, contents).map_err(|error| error.to_string())
}

fn run_command<I, S>(program: &str, args: I) -> Result<LinterRun, String>
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

    let output = match Command::new(program).args(&args).output() {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(LinterRun::skipped(program, format!("{program} not found")));
        }
        Err(error) => return Err(error.to_string()),
    };
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

fn suppress_known_cfn_lint_false_positives(mut result: LinterRun) -> LinterRun {
    if matches!(result.status, LinterStatus::Failed(_))
        && is_apigateway_tagresource_false_positive_only(&result.stdout)
    {
        result.status = LinterStatus::Passed;
    }
    result
}

fn is_apigateway_tagresource_false_positive_only(stdout: &str) -> bool {
    let blocks = stdout
        .split("\n\n")
        .map(str::trim)
        .filter(|block| !block.is_empty())
        .collect::<Vec<_>>();

    !blocks.is_empty()
        && blocks.iter().all(|block| {
            let mut lines = block.lines();
            matches!(
                lines.next(),
                Some(line) if line.starts_with("W3037 'tagresource' is not one of ")
            ) && matches!(lines.next(), Some(line) if line.contains("template.yaml:"))
                && lines.next().is_none()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suppresses_only_apigateway_tagresource_false_positive() {
        let output =
            "W3037 'tagresource' is not one of ['post']\n/tmp/.tmp123/template.yaml:12:9\n\n";

        let result = suppress_known_cfn_lint_false_positives(LinterRun {
            tool: "cfn-lint".to_string(),
            command: "cfn-lint template.yaml".to_string(),
            status: LinterStatus::Failed(Some(4)),
            stdout: output.to_string(),
            stderr: String::new(),
        });

        assert_eq!(result.status, LinterStatus::Passed);
    }

    #[test]
    fn keeps_other_cfn_lint_failures() {
        let output =
            "W3037 'totallywrong' is not one of ['post']\n/tmp/.tmp123/template.yaml:12:9\n\n";

        let result = suppress_known_cfn_lint_false_positives(LinterRun {
            tool: "cfn-lint".to_string(),
            command: "cfn-lint template.yaml".to_string(),
            status: LinterStatus::Failed(Some(4)),
            stdout: output.to_string(),
            stderr: String::new(),
        });

        assert_eq!(result.status, LinterStatus::Failed(Some(4)));
    }
}
