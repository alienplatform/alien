// Keep the shared typed error and its source chain intact at each operation boundary.
#![allow(clippy::result_large_err)]

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use alien_bindings::traits::{CommandOutput, CreateSandboxRequest, RunCommandRequest, Sandbox};
use alien_bindings::Bindings;
use alien_error::{AlienError, Context, IntoAlienError};
use alien_sandbox_conformance::{evaluate, Expectation, Observation, Probe, Verdict, GUEST_PROBE};
use futures::StreamExt;
use serde::{Deserialize, Serialize};

mod error;
use error::{operation, ErrorData, Result};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Suite {
    backend: String,
    revision: String,
    mode: String,
    subject_binding: String,
    control_binding: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Case {
    name: String,
    control: Probe,
    subject: Probe,
    expected: Expectation,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Session {
    binding: String,
    sandbox_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    backend: String,
    revision: String,
    mode: String,
    cases: Vec<CaseResult>,
    termination_calls_completed: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CaseResult {
    name: String,
    control: Observation,
    subject: Observation,
    verdict: Verdict,
}

fn invalid(message: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::Invalid {
        message: message.into(),
    })
}

async fn probe(sandbox: &dyn Sandbox, id: &str, request: &Probe) -> Result<Observation> {
    let argument = serde_json::to_string(request)
        .into_alien_error()
        .context(operation("encode guest probe"))?;
    let command = RunCommandRequest {
        command: "python3".into(),
        args: vec!["-c".into(), GUEST_PROBE.into(), argument],
        cwd: None,
        env: BTreeMap::new(),
        timeout: Duration::from_secs(10),
    };
    tokio::time::timeout(Duration::from_secs(30), async {
        let mut stream = sandbox
            .run_command(id, command)
            .await
            .context(operation("start guest probe"))?;
        let mut stdout = Vec::new();
        let mut exit = None;
        while let Some(frame) = stream.next().await {
            if exit.is_some() {
                return Err(invalid("guest emitted a frame after its terminal result"));
            }
            match frame.context(operation("read guest probe frame"))? {
                CommandOutput::Stdout { data, .. } => {
                    if stdout.len() + data.len() > 8192 {
                        return Err(invalid(
                            "guest probe exceeded its 8192-byte observation limit",
                        ));
                    }
                    stdout.extend(data);
                }
                // Diagnostic stderr may contain response details; never copy it to evidence.
                CommandOutput::Stderr { .. } => {}
                CommandOutput::Exit { code, truncated } => exit = Some((code, truncated)),
            }
        }
        if exit != Some((0, false)) {
            return Err(invalid(format!(
                "guest probe failed or was truncated: {exit:?}"
            )));
        }
        serde_json::from_slice(&stdout)
            .into_alien_error()
            .context(operation("decode guest observation"))
    })
    .await
    .into_alien_error()
    .context(operation("wait for guest probe within 30 seconds"))?
}

async fn create(
    sandbox: &dyn Sandbox,
    binding: &str,
    journal: &mut File,
    sessions: &mut Vec<Session>,
) -> Result<usize> {
    let instance = sandbox
        .create(CreateSandboxRequest::default())
        .await
        .context(operation(format!("create {binding}")))?;
    let index = sessions.len();
    sessions.push(Session {
        binding: binding.into(),
        sandbox_id: instance.sandbox_id,
    });
    // Retain the session in memory before journaling so a disk failure still triggers cleanup.
    let mut record = serde_json::to_vec(&sessions[index])
        .into_alien_error()
        .context(operation("encode cleanup journal entry"))?;
    record.push(b'\n');
    journal
        .write_all(&record)
        .into_alien_error()
        .context(operation("append cleanup journal"))?;
    journal
        .sync_all()
        .into_alien_error()
        .context(operation("persist cleanup journal"))?;
    Ok(index)
}

async fn cleanup(
    providers: &BTreeMap<String, Arc<dyn Sandbox>>,
    sessions: &[Session],
) -> Result<()> {
    let mut failure = None;
    let mut failures = 0;
    for session in sessions.iter().rev() {
        let result = async {
            let sandbox = providers
                .get(&session.binding)
                .ok_or_else(|| invalid("missing cleanup provider"))?;
            sandbox
                .terminate(&session.sandbox_id)
                .await
                .context(operation(format!(
                    "terminate {} in {}",
                    session.sandbox_id, session.binding
                )))
        }
        .await;
        if let Err(error) = result {
            failures += 1;
            eprintln!(
                "Cleanup failed for {} in {}; journal retained",
                session.sandbox_id, session.binding
            );
            failure.get_or_insert(error);
        }
    }
    match failure {
        Some(error) => Err(error).context(operation(format!(
            "cleanup failed for {failures} session(s); rerun cleanup with the retained journal"
        ))),
        None => Ok(()),
    }
}

async fn run(bindings: &Bindings, suite: Suite, journal_path: &Path) -> Result<Report> {
    if suite.cases.is_empty() || suite.subject_binding == suite.control_binding {
        return Err(invalid(
            "supply probe cases and distinct subject/control bindings",
        ));
    }
    let mut journal = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(journal_path)
        .into_alien_error()
        .context(operation(
            "create a fresh cleanup journal (never overwrite a previous run)",
        ))?;
    let providers = BTreeMap::from([
        (
            suite.control_binding.clone(),
            bindings
                .sandbox(&suite.control_binding)
                .await
                .context(operation("load control binding"))?,
        ),
        (
            suite.subject_binding.clone(),
            bindings
                .sandbox(&suite.subject_binding)
                .await
                .context(operation("load subject binding"))?,
        ),
    ]);
    let control_provider = providers
        .get(&suite.control_binding)
        .ok_or_else(|| invalid("missing control provider"))?;
    let subject_provider = providers
        .get(&suite.subject_binding)
        .ok_or_else(|| invalid("missing subject provider"))?;
    let mut sessions = Vec::new();
    let result = async {
        let control = create(
            control_provider.as_ref(),
            &suite.control_binding,
            &mut journal,
            &mut sessions,
        )
        .await?;
        let subject = create(
            subject_provider.as_ref(),
            &suite.subject_binding,
            &mut journal,
            &mut sessions,
        )
        .await?;
        let mut results = Vec::new();
        for case in suite.cases {
            let control = probe(
                control_provider.as_ref(),
                &sessions[control].sandbox_id,
                &case.control,
            )
            .await?;
            let subject = probe(
                subject_provider.as_ref(),
                &sessions[subject].sandbox_id,
                &case.subject,
            )
            .await?;
            let verdict = evaluate(&case.expected, &control, &subject);
            results.push(CaseResult {
                name: case.name,
                control,
                subject,
                verdict,
            });
        }
        Ok(Report {
            backend: suite.backend,
            revision: suite.revision,
            mode: suite.mode,
            cases: results,
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
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Err(run_error), Err(cleanup_error)) => Err(cleanup_error).context(operation(format!(
            "run also failed with {}; cleanup remains incomplete",
            run_error.code
        ))),
    }
}

async fn execute() -> Result<bool> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let bindings = Bindings::from_env().context(operation("read deployment bindings"))?;
    match args.as_slice() {
        [action, suite_path, journal_path] if action == "run" => {
            let file = File::open(suite_path)
                .into_alien_error()
                .context(operation("open suite configuration"))?;
            let suite = serde_json::from_reader(file)
                .into_alien_error()
                .context(operation("decode suite configuration"))?;
            let report = run(&bindings, suite, Path::new(journal_path)).await?;
            let passed = report
                .cases
                .iter()
                .all(|case| case.verdict == Verdict::Pass);
            serde_json::to_writer_pretty(std::io::stdout().lock(), &report)
                .into_alien_error()
                .context(operation("write conformance report"))?;
            println!();
            Ok(passed)
        }
        [action, journal_path] if action == "cleanup" => {
            let file = File::open(journal_path)
                .into_alien_error()
                .context(operation("open cleanup journal"))?;
            let mut sessions: Vec<Session> = Vec::new();
            for line in BufReader::new(file).lines() {
                let line = line
                    .into_alien_error()
                    .context(operation("read cleanup journal"))?;
                sessions.push(
                    serde_json::from_str(&line)
                        .into_alien_error()
                        .context(operation("decode cleanup journal entry"))?,
                );
            }
            let mut providers = BTreeMap::new();
            for session in &sessions {
                let provider = bindings
                    .sandbox(&session.binding)
                    .await
                    .context(operation("load recovery binding"))?;
                // This backend stores claims in the original provider instance. A fresh
                // instance silently treats terminate as a no-op, so recovery cannot use it.
                #[cfg(feature = "kubernetes")]
                if provider
                    .as_any()
                    .is::<alien_bindings::providers::sandbox::kubernetes::KubernetesSandbox>()
                {
                    return Err(invalid("Kubernetes session recovery requires deleting the recorded session through its broker; a new binding cannot recover its claim"));
                }
                providers.insert(session.binding.clone(), provider);
            }
            cleanup(&providers, &sessions).await?;
            Ok(true)
        }
        _ => Err(invalid(
            "usage: alien-sandbox-conformance run SUITE.json JOURNAL.jsonl | cleanup JOURNAL.jsonl",
        )),
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    match execute().await {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
