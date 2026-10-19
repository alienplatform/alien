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
    // Only a finished run can be suppressed: cfn-lint reported findings on stdout
    // and wrote nothing to stderr. An empty or stderr-only run is a crash.
    if !matches!(result.status, LinterStatus::Failed(_))
        || result.stdout.trim().is_empty()
        || !result.stderr.trim().is_empty()
    {
        return result;
    }
    let remaining = without_kept_setup_parameter_warnings(&result.stdout);
    if remaining.trim().is_empty()
        || is_apigateway_tagresource_false_positive_only(&remaining)
        || is_bedrock_mantle_service_prefix_false_positive_only(&remaining)
        || is_nextgen_opensearch_schema_lag_only(&remaining)
        || is_lambda_microvm_schema_lag_only(&remaining)
    {
        result.status = LinterStatus::Passed;
    }
    result
}

/// Setup parameters declared even by stacks that do not read them, so the
/// parameter list an installed stack was created with stays valid on updates
/// (CloudFormation rejects an update that passes a parameter the template no
/// longer declares):
///
/// - `ManagingAccountId` is read by no built-in emitter, only by extension emitters;
/// - `NetworkMode` is read only when the stack has a network;
/// - `HostedZoneId` is part of the custom-domain inputs but nothing reads it;
/// - `ManagingRoleArn` is not read by Kubernetes targets, which register no
///   management role.
///
/// cfn-lint did not report them while templates used the AWS::LanguageExtensions
/// transform. Any other unused parameter fails the lint.
const KEPT_SETUP_PARAMETERS: [&str; 4] = [
    "ManagingAccountId",
    "NetworkMode",
    "HostedZoneId",
    "ManagingRoleArn",
];

/// The linter output without its W2001 warnings for [`KEPT_SETUP_PARAMETERS`].
fn without_kept_setup_parameter_warnings(stdout: &str) -> String {
    stdout
        .split("\n\n")
        .map(str::trim)
        .filter(|block| !block.is_empty())
        .filter(|block| {
            let mut lines = block.lines();
            let kept = matches!(
                lines.next(),
                Some(line) if KEPT_SETUP_PARAMETERS
                    .iter()
                    .any(|name| line == format!("W2001 Parameter {name} not used."))
            );
            let located = matches!(lines.next(), Some(line) if line.contains("template.yaml:"));
            !(kept && located && lines.next().is_none())
        })
        .map(|block| format!("{block}\n\n"))
        .collect()
}

// cfn-lint's IAM catalogue predates these documented Lambda actions. Additional
// template conditions can cause it to inspect statements it previously skipped.
// Keep this exception exact: unrelated W3037 diagnostics and all errors still fail.
// https://docs.aws.amazon.com/lambda/latest/dg/microvms-launching.html
fn is_lambda_microvm_schema_lag_only(stdout: &str) -> bool {
    let blocks: Vec<_> = stdout
        .split("\n\n")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    !blocks.is_empty()
        && blocks.iter().all(|block| {
            let mut lines = block.lines();
            let Some(warning) = lines.next() else {
                return false;
            };
            let known_action = [
                "runmicrovm",
                "suspendmicrovm",
                "resumemicrovm",
                "terminatemicrovm",
                "createmicrovmauthtoken",
                "getmicrovm",
                "getmicrovmimage",
                "passnetworkconnector",
            ]
            .iter()
            .any(|action| warning.starts_with(&format!("W3037 '{action}' is not one of ")));
            known_action
                && matches!(lines.next(), Some(line) if line.contains("template.yaml:"))
                && lines.next().is_none()
        })
}

fn is_bedrock_mantle_service_prefix_false_positive_only(stdout: &str) -> bool {
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
                Some(line) if line.starts_with("W3037 'bedrock-mantle' is not one of ")
            ) && matches!(lines.next(), Some(line) if line.contains("template.yaml:"))
                && lines.next().is_none()
        })
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

fn is_nextgen_opensearch_schema_lag_only(stdout: &str) -> bool {
    let blocks = stdout
        .split("\n\n")
        .map(str::trim)
        .filter(|block| !block.is_empty())
        .collect::<Vec<_>>();

    !blocks.is_empty()
        && blocks.iter().all(|block| {
            let mut lines = block.lines();
            let known_error = matches!(
                lines.next(),
                Some(
                    "E3006 Resource type 'AWS::OpenSearchServerless::CollectionGroup' does not exist in 'us-east-1'"
                        | "E3002 Additional properties are not allowed ('CollectionGroupName' was unexpected)"
                        | "E3002 Additional properties are not allowed ('EncryptionConfig' was unexpected)"
                )
            );
            known_error
                && matches!(lines.next(), Some(line) if line.contains("template.yaml:"))
                && lines.next().is_none()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn microvm_catalogue_exception_never_hides_unknown_actions_or_other_errors() {
        let known = "W3037 'runmicrovm' is not one of ['invoke']\n/tmp/template.yaml:10:1\n\n";
        assert!(is_lambda_microvm_schema_lag_only(known));
        assert!(!is_lambda_microvm_schema_lag_only(""));
        assert!(!is_lambda_microvm_schema_lag_only(
            &known.replace("runmicrovm", "typo")
        ));
        assert!(!is_lambda_microvm_schema_lag_only(&format!(
            "{known}E3032 empty list\n/tmp/template.yaml:11:1\n\n"
        )));
    }

    #[test]
    fn unused_parameter_exception_covers_only_kept_setup_parameters() {
        let failed = |stdout: &str| {
            suppress_known_cfn_lint_false_positives(LinterRun {
                tool: "cfn-lint".to_string(),
                command: "cfn-lint template.yaml".to_string(),
                status: LinterStatus::Failed(Some(4)),
                stdout: stdout.to_string(),
                stderr: String::new(),
            })
            .status
        };
        let kept = "W2001 Parameter ManagingAccountId not used.\n/tmp/template.yaml:10:3\n\n\
W2001 Parameter NetworkMode not used.\n/tmp/template.yaml:20:3\n\n";
        assert_eq!(failed(kept), LinterStatus::Passed);
        assert_eq!(
            failed(&format!(
                "{kept}W3037 'tagresource' is not one of ['post']\n/tmp/template.yaml:12:9\n\n"
            )),
            LinterStatus::Passed
        );
        assert_eq!(
            failed(&kept.replace("NetworkMode", "VpcId")),
            LinterStatus::Failed(Some(4))
        );
        assert_eq!(
            failed(&format!(
                "{kept}E3032 empty list\n/tmp/template.yaml:11:1\n\n"
            )),
            LinterStatus::Failed(Some(4))
        );
    }

    #[test]
    fn a_lint_run_that_did_not_finish_is_never_suppressed() {
        let failed = |stdout: &str, stderr: &str| {
            suppress_known_cfn_lint_false_positives(LinterRun {
                tool: "cfn-lint".to_string(),
                command: "cfn-lint template.yaml".to_string(),
                status: LinterStatus::Failed(Some(1)),
                stdout: stdout.to_string(),
                stderr: stderr.to_string(),
            })
            .status
        };
        let kept = "W2001 Parameter NetworkMode not used.\n/tmp/template.yaml:20:3\n\n";
        let crash = "Traceback (most recent call last):\nValueError: bad template\n";
        assert_eq!(failed("", ""), LinterStatus::Failed(Some(1)));
        assert_eq!(failed("", crash), LinterStatus::Failed(Some(1)));
        assert_eq!(failed(kept, crash), LinterStatus::Failed(Some(1)));
    }

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

    #[test]
    fn suppresses_only_bedrock_mantle_service_prefix_schema_lag() {
        let output =
            "W3037 'bedrock-mantle' is not one of ['bedrock']\n/tmp/.tmp123/template.yaml:12:9\n\n";

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
    fn suppresses_only_nextgen_opensearch_schema_lag() {
        let output = "E3006 Resource type 'AWS::OpenSearchServerless::CollectionGroup' does not exist in 'us-east-1'\n/tmp/.tmp123/template.yaml:12:5\n\n\
E3002 Additional properties are not allowed ('CollectionGroupName' was unexpected)\n/tmp/.tmp123/template.yaml:24:7\n\n\
E3002 Additional properties are not allowed ('EncryptionConfig' was unexpected)\n/tmp/.tmp123/template.yaml:27:7\n\n";

        let result = suppress_known_cfn_lint_false_positives(LinterRun {
            tool: "cfn-lint".to_string(),
            command: "cfn-lint template.yaml".to_string(),
            status: LinterStatus::Failed(Some(2)),
            stdout: output.to_string(),
            stderr: String::new(),
        });

        assert_eq!(result.status, LinterStatus::Passed);
    }
}
