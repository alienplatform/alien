//! Interactive command conformance: the same cases against every backend that claims
//! `interactiveCommands`.
//!
//! Each case drives a real program through the binding an application would use and judges only
//! what the caller can observe: the bytes that came back, when they came back, and the one
//! terminal item. The cases need `/bin/sh` and coreutils in the image; the REPL case also needs
//! `python3` and is skipped, and reported as skipped, without it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use alien_bindings::error::ErrorData as BindingError;
use alien_bindings::traits::{
    CommandOutput, CommandSignal, InteractiveCommand, RunCommandRequest, Sandbox,
};
use futures::StreamExt;
use serde::Serialize;

/// How long any single expected output may take to arrive. Generous, because a cold sandbox
/// starts its first process slowly; the cases measure ordering, not speed.
const OUTPUT_WAIT: Duration = Duration::from_secs(30);

/// The outcome of one case.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaseResult {
    pub name: String,
    pub verdict: CaseVerdict,
    /// What was observed, for the evidence record
    pub detail: String,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CaseVerdict {
    Pass,
    Fail,
    /// The case's precondition does not hold in this image; nothing was claimed.
    Skipped,
}

type Outcome = std::result::Result<String, String>;

/// Runs every interactive case against one sandbox session.
pub async fn run_cases(sandbox: Arc<dyn Sandbox>, sandbox_id: &str) -> Vec<CaseResult> {
    let mut results = Vec::new();
    macro_rules! case {
        ($name:literal, $body:expr) => {{
            let started = Instant::now();
            let outcome: Outcome = $body.await;
            results.push(CaseResult {
                name: $name.to_string(),
                verdict: if outcome.is_ok() {
                    CaseVerdict::Pass
                } else {
                    CaseVerdict::Fail
                },
                detail: outcome.unwrap_or_else(|error| error),
                elapsed_ms: started.elapsed().as_millis(),
            });
        }};
    }

    case!("capabilityClaimed", async {
        if sandbox.capabilities().interactive_commands {
            Ok("interactiveCommands is true".to_string())
        } else {
            Err("the backend does not claim interactiveCommands".to_string())
        }
    });
    case!(
        "promptWithoutNewlineThenTurns",
        prompt_turns(&*sandbox, sandbox_id)
    );
    case!("catUntilEof", cat_until_eof(&*sandbox, sandbox_id));
    case!("interruptReachesProgram", interrupt(&*sandbox, sandbox_id));
    case!(
        "largeInputWaitsForReader",
        large_input(&*sandbox, sandbox_id)
    );
    case!(
        "signalAfterExitIsRefused",
        signal_after_exit(&*sandbox, sandbox_id)
    );
    case!(
        "droppedOutputKillsCommand",
        dropped_output_kills(&*sandbox, sandbox_id)
    );
    case!(
        "signalRightAfterStart",
        signal_right_after_start(&*sandbox, sandbox_id)
    );
    case!(
        "deadlineHoldsWhileNotReading",
        deadline_without_reader(&*sandbox, sandbox_id)
    );

    case!(
        "concurrentStartsStopAtTheLimit",
        concurrent_starts(&*sandbox, sandbox_id)
    );

    let started = Instant::now();
    let repl = python_repl(&*sandbox, sandbox_id).await;
    results.push(CaseResult {
        name: "pythonRepl".to_string(),
        verdict: match &repl {
            Ok(Some(_)) => CaseVerdict::Pass,
            Ok(None) => CaseVerdict::Skipped,
            Err(_) => CaseVerdict::Fail,
        },
        detail: match repl {
            Ok(Some(detail)) => detail,
            Ok(None) => "python3 is not in the image".to_string(),
            Err(error) => error,
        },
        elapsed_ms: started.elapsed().as_millis(),
    });

    results
}

fn sh(script: &str) -> RunCommandRequest {
    RunCommandRequest {
        command: "/bin/sh".to_string(),
        args: vec!["-c".to_string(), script.to_string()],
        cwd: None,
        env: Default::default(),
        timeout: Duration::from_secs(120),
    }
}

/// A command's output as the caller has seen it so far.
struct Driven {
    command: InteractiveCommand,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    terminal: Option<std::result::Result<CommandOutput, String>>,
    terminals: usize,
}

impl Driven {
    async fn start(
        sandbox: &dyn Sandbox,
        id: &str,
        request: RunCommandRequest,
    ) -> Result<Self, String> {
        let command = sandbox
            .run_interactive(id, request)
            .await
            .map_err(|error| format!("start: {error}"))?;
        Ok(Self {
            command,
            stdout: Vec::new(),
            stderr: Vec::new(),
            terminal: None,
            terminals: 0,
        })
    }

    fn take(&mut self, item: alien_bindings::error::Result<CommandOutput>) {
        match item {
            Ok(CommandOutput::Stdout { data, .. }) => self.stdout.extend(data),
            Ok(CommandOutput::Stderr { data, .. }) => self.stderr.extend(data),
            Ok(exit @ CommandOutput::Exit { .. }) => {
                self.terminals += 1;
                self.terminal = Some(Ok(exit));
            }
            Err(error) => {
                self.terminals += 1;
                self.terminal = Some(Err(error.to_string()));
            }
        }
    }

    /// Reads until `stream` (stdout or stderr) ends with `expected`.
    async fn until(&mut self, expected: &str, on_stderr: bool) -> Result<(), String> {
        let deadline = tokio::time::Instant::now() + OUTPUT_WAIT;
        loop {
            let seen = if on_stderr {
                &self.stderr
            } else {
                &self.stdout
            };
            if String::from_utf8_lossy(seen).ends_with(expected) {
                return Ok(());
            }
            if self.terminal.is_some() {
                return Err(format!(
                    "ended before {expected:?}; stdout {:?}",
                    String::from_utf8_lossy(&self.stdout)
                ));
            }
            match tokio::time::timeout_at(deadline, self.command.output.next()).await {
                Err(_) => {
                    return Err(format!(
                        "{expected:?} did not arrive within {}s; stdout {:?}",
                        OUTPUT_WAIT.as_secs(),
                        String::from_utf8_lossy(&self.stdout)
                    ))
                }
                Ok(None) => return Err("the stream ended without a terminal item".to_string()),
                Ok(Some(item)) => self.take(item),
            }
        }
    }

    /// Reads to the end and returns the exit code, holding the one-terminal-item rule.
    async fn finish(&mut self) -> Result<i32, String> {
        let deadline = tokio::time::Instant::now() + OUTPUT_WAIT;
        loop {
            match tokio::time::timeout_at(deadline, self.command.output.next()).await {
                Err(_) => return Err("the command did not end".to_string()),
                Ok(None) => break,
                Ok(Some(item)) => {
                    if self.terminal.is_some() {
                        return Err("an item followed the terminal item".to_string());
                    }
                    self.take(item);
                }
            }
        }
        if self.terminals != 1 {
            return Err(format!(
                "{} terminal items, not exactly one",
                self.terminals
            ));
        }
        match self.terminal.take().expect("counted") {
            Ok(CommandOutput::Exit { code, .. }) => Ok(code),
            Ok(other) => Err(format!("unexpected terminal {other:?}")),
            Err(error) => Err(format!("ended with an error: {error}")),
        }
    }

    fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
}

/// A prompt with no newline reaches the caller while the program waits, and each answer comes
/// back before the next input is sent.
async fn prompt_turns(sandbox: &dyn Sandbox, id: &str) -> Outcome {
    let mut driven = Driven::start(
        sandbox,
        id,
        sh("printf 'name? '; while read n; do echo \"hello $n\"; printf 'name? '; done"),
    )
    .await?;
    driven.until("name? ", false).await?;
    for name in ["ada", "grace", "linus"] {
        driven
            .command
            .input
            .write(format!("{name}\n").as_bytes())
            .await
            .map_err(|error| format!("write: {error}"))?;
        driven
            .until(&format!("hello {name}\nname? "), false)
            .await?;
    }
    driven
        .command
        .input
        .close_stdin()
        .await
        .map_err(|error| format!("close: {error}"))?;
    let code = driven.finish().await?;
    let expected = "name? hello ada\nname? hello grace\nname? hello linus\nname? ";
    if code != 0 || driven.stdout() != expected {
        return Err(format!("exit {code}, stdout {:?}", driven.stdout()));
    }
    Ok(format!("3 turns; stdout {:?}; exit 0", driven.stdout()))
}

/// Input in several writes arrives in order, and closing stdin ends a program reading to EOF.
async fn cat_until_eof(sandbox: &dyn Sandbox, id: &str) -> Outcome {
    let mut driven = Driven::start(sandbox, id, sh("cat")).await?;
    for piece in ["one\n", "two", "\nthree"] {
        driven
            .command
            .input
            .write(piece.as_bytes())
            .await
            .map_err(|error| format!("write: {error}"))?;
    }
    driven
        .command
        .input
        .close_stdin()
        .await
        .map_err(|error| format!("close: {error}"))?;
    let code = driven.finish().await?;
    if code != 0 || driven.stdout() != "one\ntwo\nthree" {
        return Err(format!("exit {code}, stdout {:?}", driven.stdout()));
    }
    Ok("stdout \"one\\ntwo\\nthree\"; exit 0 after close".to_string())
}

/// An interrupt reaches the running program, which handles it as it chooses.
async fn interrupt(sandbox: &dyn Sandbox, id: &str) -> Outcome {
    let mut driven = Driven::start(
        sandbox,
        id,
        sh("trap 'echo interrupted; exit 130' INT; echo ready; while :; do sleep 1; done"),
    )
    .await?;
    driven.until("ready\n", false).await?;
    driven
        .command
        .input
        .signal(CommandSignal::Interrupt)
        .await
        .map_err(|error| format!("signal: {error}"))?;
    let code = driven.finish().await?;
    if code != 130 || !driven.stdout().ends_with("interrupted\n") {
        return Err(format!("exit {code}, stdout {:?}", driven.stdout()));
    }
    Ok("trap ran; exit 130".to_string())
}

/// Input far larger than any pipe holds, to a program that starts reading only after 3 seconds:
/// the write waits for the reader rather than completing into a buffer, and every byte arrives.
async fn large_input(sandbox: &dyn Sandbox, id: &str) -> Outcome {
    const BYTES: usize = 8 << 20;
    let mut driven = Driven::start(sandbox, id, sh("sleep 3; wc -c")).await?;
    let started = Instant::now();
    driven
        .command
        .input
        .write(&vec![b'x'; BYTES])
        .await
        .map_err(|error| format!("write: {error}"))?;
    let waited = started.elapsed();
    driven
        .command
        .input
        .close_stdin()
        .await
        .map_err(|error| format!("close: {error}"))?;
    let code = driven.finish().await?;
    let counted = driven.stdout().trim().to_string();
    if code != 0 || counted != BYTES.to_string() {
        return Err(format!("exit {code}, counted {counted:?}"));
    }
    if waited < Duration::from_secs(2) {
        return Err(format!(
            "the {BYTES}-byte write finished after {}ms, before the program read anything",
            waited.as_millis()
        ));
    }
    Ok(format!(
        "{BYTES} bytes counted; the write waited {}ms for the reader",
        waited.as_millis()
    ))
}

/// Once the command has ended, a signal is refused as naming nothing rather than reaching some
/// other process.
async fn signal_after_exit(sandbox: &dyn Sandbox, id: &str) -> Outcome {
    let mut driven = Driven::start(sandbox, id, sh("exit 3")).await?;
    let code = driven.finish().await?;
    if code != 3 {
        return Err(format!("exit {code}"));
    }
    match driven.command.input.signal(CommandSignal::Kill).await {
        Ok(()) => Err("a signal after exit was accepted".to_string()),
        Err(error) => match &error.error {
            Some(BindingError::SandboxCommandFailed { failure, .. })
                if failure == "commandNotRunning" =>
            {
                Ok("exit 3; a later signal is commandNotRunning".to_string())
            }
            _ => Err(format!("refused with the wrong error: {error}")),
        },
    }
}

/// Dropping the output stream ends the command, checked from a second command.
async fn dropped_output_kills(sandbox: &dyn Sandbox, id: &str) -> Outcome {
    let marker = format!("interactive-drop-{}", std::process::id());
    let mut driven = Driven::start(
        sandbox,
        id,
        sh(&format!("echo $$ > {marker}; echo started; exec sleep 300")),
    )
    .await?;
    driven.until("started\n", false).await?;
    drop(driven);

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let check = run(
            sandbox,
            id,
            sh(&format!(
                "kill -0 \"$(cat {marker})\" 2>/dev/null && echo alive || echo gone"
            )),
        )
        .await?;
        if check.trim() == "gone" {
            let _ = run(sandbox, id, sh(&format!("rm -f {marker}"))).await;
            return Ok("the process was gone after the output was dropped".to_string());
        }
        if Instant::now() > deadline {
            return Err("the command outlived its dropped output by 20s".to_string());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// A signal sent the moment the start returns reaches the command: the start answers only once
/// the command is running and can be signalled.
async fn signal_right_after_start(sandbox: &dyn Sandbox, id: &str) -> Outcome {
    let mut driven = Driven::start(sandbox, id, sh("exec sleep 300")).await?;
    driven
        .command
        .input
        .signal(CommandSignal::Kill)
        .await
        .map_err(|error| format!("signal: {error}"))?;
    let code = driven.finish().await?;
    Ok(format!("signalled immediately; ended with exit {code}"))
}

/// A caller that holds the output open without reading must not hold the command past its
/// timeout: checked from a second command while the first one's output is still unread.
async fn deadline_without_reader(sandbox: &dyn Sandbox, id: &str) -> Outcome {
    let marker = format!("interactive-deadline-{}", std::process::id());
    let mut request = sh(&format!(
        "echo $$ > {marker}; exec yes interactive-deadline"
    ));
    request.timeout = Duration::from_secs(3);
    let mut driven = Driven::start(sandbox, id, request).await?;

    // Not reading: `yes` fills every buffer between it and this caller within the timeout.
    tokio::time::sleep(Duration::from_secs(8)).await;
    let check = run(
        sandbox,
        id,
        sh(&format!(
            "kill -0 \"$(cat {marker})\" 2>/dev/null && echo alive || echo gone; rm -f {marker}"
        )),
    )
    .await?;
    if check.trim() != "gone" {
        return Err("the command was still running 5s past its timeout while unread".to_string());
    }

    // Draining now must end in the timeout, not in an exit.
    let deadline = tokio::time::Instant::now() + OUTPUT_WAIT;
    loop {
        match tokio::time::timeout_at(deadline, driven.command.output.next()).await {
            Err(_) => return Err("the stream did not end after the deadline".to_string()),
            Ok(None) => return Err("the stream ended without a terminal item".to_string()),
            Ok(Some(Ok(CommandOutput::Exit { code, .. }))) => {
                return Err(format!("ended with exit {code}, not the timeout"))
            }
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(error))) => {
                return if error.to_string().contains("timeoutExceeded") {
                    Ok("gone 5s past its timeout while unread; the stream ends with timeoutExceeded".to_string())
                } else {
                    Err(format!("ended with {error}"))
                }
            }
        }
    }
}

/// A real REPL: the interpreter's prompt arrives on stderr with no newline, an expression's
/// value comes back on stdout, and end of input ends the interpreter.
async fn python_repl(sandbox: &dyn Sandbox, id: &str) -> Result<Option<String>, String> {
    let present = run(
        sandbox,
        id,
        sh("command -v python3 >/dev/null && echo yes || echo no"),
    )
    .await?;
    if present.trim() != "yes" {
        return Ok(None);
    }
    let mut driven = Driven::start(
        sandbox,
        id,
        RunCommandRequest {
            command: "python3".to_string(),
            args: vec!["-q".to_string(), "-u".to_string(), "-i".to_string()],
            cwd: None,
            env: Default::default(),
            timeout: Duration::from_secs(120),
        },
    )
    .await?;
    driven.until(">>> ", true).await?;
    driven
        .command
        .input
        .write(b"x = 6 * 7\nprint(x)\n")
        .await
        .map_err(|error| format!("write: {error}"))?;
    driven.until("42\n", false).await?;
    driven
        .command
        .input
        .close_stdin()
        .await
        .map_err(|error| format!("close: {error}"))?;
    let code = driven.finish().await?;
    if code != 0 {
        return Err(format!("exit {code}"));
    }
    Ok(Some(
        "prompt on stderr, 42 on stdout, exit 0 at EOF".to_string(),
    ))
}

/// How many interactive commands one session runs at once, on every backend that has them.
const INTERACTIVE_LIMIT: usize = 16;

/// Starts that arrive together cannot pass the limit between them: each is admitted or refused
/// as if it had come alone, and the refused ones say why.
async fn concurrent_starts(sandbox: &dyn Sandbox, id: &str) -> Outcome {
    let attempts = INTERACTIVE_LIMIT + 4;
    let starts = (0..attempts).map(|_| sandbox.run_interactive(id, sh("exec sleep 300")));
    let results = futures::future::join_all(starts).await;

    let mut running = Vec::new();
    let mut refused = 0;
    for result in results {
        match result {
            Ok(command) => running.push(command),
            Err(error) if error.to_string().contains("INTERACTIVE_LIMIT_REACHED") => refused += 1,
            Err(error) => return Err(format!("a start failed for another reason: {error}")),
        }
    }
    let admitted = running.len();
    // Dropping the output kills each command and frees its slot.
    drop(running);
    if admitted != INTERACTIVE_LIMIT || refused != attempts - INTERACTIVE_LIMIT {
        return Err(format!(
            "{attempts} concurrent starts: {admitted} admitted and {refused} refused, expected \
             {INTERACTIVE_LIMIT} and {}",
            attempts - INTERACTIVE_LIMIT
        ));
    }

    // The slots come back once the commands are gone, so later cases start in an empty session.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let probes = (0..INTERACTIVE_LIMIT).map(|_| sandbox.run_interactive(id, sh("exit 0")));
        let probes = futures::future::join_all(probes).await;
        let all_admitted = probes.iter().all(Result::is_ok);
        // Read to the end, so each probe has finished and given its slot back too.
        for mut probe in probes.into_iter().flatten() {
            while probe.output.next().await.is_some() {}
        }
        if all_admitted {
            break;
        }
        if Instant::now() > deadline {
            return Err("the slots were not released 30s after the commands were dropped".into());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Ok(format!(
        "{attempts} concurrent starts: {admitted} admitted, {refused} refused with \
         INTERACTIVE_LIMIT_REACHED; all slots free again after the drop"
    ))
}

/// Runs a short command and returns its stdout.
async fn run(
    sandbox: &dyn Sandbox,
    id: &str,
    request: RunCommandRequest,
) -> Result<String, String> {
    let mut stream = sandbox
        .run_command(id, request)
        .await
        .map_err(|error| format!("run: {error}"))?;
    let mut stdout = Vec::new();
    while let Some(item) = stream.next().await {
        match item.map_err(|error| format!("run: {error}"))? {
            CommandOutput::Stdout { data, .. } => stdout.extend(data),
            CommandOutput::Stderr { .. } | CommandOutput::Exit { .. } => {}
        }
    }
    Ok(String::from_utf8_lossy(&stdout).into_owned())
}
