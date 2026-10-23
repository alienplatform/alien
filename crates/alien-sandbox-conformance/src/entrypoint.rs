//! Boot/USER conformance through the same bindings applications use.
use std::{collections::BTreeMap, fs::OpenOptions, path::Path, time::Duration};

use alien_bindings::{
    traits::{CommandOutput, RunCommandRequest},
    Bindings,
};
use alien_error::{Context, IntoAlienError};
use futures::StreamExt;
use serde::{Deserialize, Serialize};

use super::{cleanup, create, invalid, operation, Result};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Suite {
    backend: String,
    revision: String,
    binding: String,
    /// Reconnect the created session through a different release binding.
    reconnect_binding: Option<String>,
    image_uid: u32,
    exec_uid: u32,
    /// Probe cloud metadata in all fixture identities when supplied.
    metadata_backend: Option<String>,
    /// Observe transparent gateway access from boot, child and exec identities.
    #[serde(default)]
    gateway: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    backend: String,
    revision: String,
    reconnect_binding: Option<String>,
    pub passed: bool,
    exit_code: Option<i32>,
    evidence: serde_json::Value,
    termination_calls_completed: bool,
}

pub async fn run(bindings: &Bindings, suite: Suite, journal_path: &Path) -> Result<Report> {
    if suite.exec_uid == 0 {
        return Err(invalid("entrypoint conformance requires non-root exec"));
    }
    let mut args = vec![
        "/opt/entrypoint-conformance/inspect.py".into(),
        "--image-uid".into(),
        suite.image_uid.to_string(),
        "--exec-uid".into(),
        suite.exec_uid.to_string(),
    ];
    if let Some(backend) = &suite.metadata_backend {
        if !matches!(backend.as_str(), "aws" | "gcp" | "azure") {
            return Err(invalid("unknown metadata backend"));
        }
        args.extend(["--metadata-backend".into(), backend.clone()]);
    }
    if suite.gateway {
        args.push("--gateway".into());
    }
    let mut journal = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(journal_path)
        .into_alien_error()
        .context(operation("create a fresh entrypoint cleanup journal"))?;
    let provider = bindings
        .sandbox(&suite.binding)
        .await
        .context(operation("load entrypoint binding"))?;
    let providers = BTreeMap::from([(suite.binding.clone(), provider.clone())]);
    let mut sessions = Vec::new();
    let result = async {
        let session = create(
            provider.as_ref(),
            &suite.binding,
            &mut journal,
            &mut sessions,
        )
        .await?;
        let command_provider = match &suite.reconnect_binding {
            Some(binding) => bindings
                .sandbox(binding)
                .await
                .context(operation("load reconnect binding"))?,
            None => provider.clone(),
        };
        // This is the first user exec. The binding's create/readiness path may only
        // use health/identity probes, never start or repair the fixture supervisor.
        let request = RunCommandRequest {
            command: "python3".into(),
            args,
            cwd: None,
            env: BTreeMap::new(),
            timeout: Duration::from_secs(120),
        };
        let (stdout, exit) = tokio::time::timeout(Duration::from_secs(135), async {
            let mut stream = command_provider
                .run_command(&sessions[session].sandbox_id, request)
                .await
                .context(operation("start image identity checker"))?;
            let mut stdout = Vec::new();
            let mut exit = None;
            while let Some(frame) = stream.next().await {
                if exit.is_some() {
                    return Err(invalid("frame after checker exit"));
                }
                match frame.context(operation("read image identity checker"))? {
                    CommandOutput::Stdout { data, .. } => {
                        if stdout.len() + data.len() > 65536 {
                            return Err(invalid("checker evidence exceeds 64 KiB"));
                        }
                        stdout.extend(data);
                    }
                    CommandOutput::Stderr { .. } => {}
                    CommandOutput::Exit { code, truncated } => exit = Some((code, truncated)),
                }
            }
            Ok((stdout, exit))
        })
        .await
        .into_alien_error()
        .context(operation("bound identity check to 135 seconds"))??;
        let evidence = if stdout.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&stdout)
                .into_alien_error()
                .context(operation("decode identity evidence"))?
        };
        Ok(Report {
            backend: suite.backend,
            revision: suite.revision,
            reconnect_binding: suite.reconnect_binding,
            passed: exit == Some((0, false)) && !evidence.is_null(),
            exit_code: exit.map(|value| value.0),
            evidence,
            termination_calls_completed: false,
        })
    }
    .await;
    let cleaned = cleanup(&providers, &sessions).await;
    match (result, cleaned) {
        (Ok(mut report), Ok(())) => {
            report.termination_calls_completed = true;
            Ok(report)
        }
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(run_error), Err(cleanup_error)) => Err(cleanup_error).context(operation(format!(
            "entrypoint check also failed with {}; cleanup remains incomplete",
            run_error.code
        ))),
    }
}
