//! Terminal output helpers for alien-deploy CLI.
//!
//! Lightweight, TTY-aware output with no heavy TUI dependencies.

use std::collections::BTreeMap;
use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use alien_core::{DeploymentStatus, ResourceStatus, StackState};
use alien_deployment::runner::StepProgress;

// ---------------------------------------------------------------------------
// TTY detection
// ---------------------------------------------------------------------------

pub fn is_tty() -> bool {
    std::io::stderr().is_terminal()
}

fn ansi(code: &str, text: &str) -> String {
    if is_tty() {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

// ---------------------------------------------------------------------------
// Basic output helpers
// ---------------------------------------------------------------------------

pub fn success(msg: &str) {
    eprintln!("{} {}", ansi("32", "✔"), msg);
}

pub fn info(msg: &str) {
    eprintln!("{} {}", ansi("34", "ℹ"), msg);
}

pub fn warn(msg: &str) {
    eprintln!("{} {}", ansi("33", "⚠"), msg);
}

pub fn error(msg: &str) {
    eprintln!("{} {}", ansi("31", "✗"), msg);
}

pub fn step(num: usize, total: usize, msg: &str) {
    eprintln!("{} {}", ansi("36", &format!("[{num}/{total}]")), msg);
}

pub fn banner(title: &str) {
    eprintln!();
    eprintln!("{}", ansi("1", title));
    eprintln!();
}

pub fn label_value(label: &str, value: &str) {
    eprintln!("  {} {}", ansi("2", &format!("{label:<14}")), value);
}

pub fn dim(msg: &str) -> String {
    ansi("2", msg)
}

pub fn bold(msg: &str) -> String {
    ansi("1", msg)
}

/// Print a status line (legacy — prefer label_value)
pub fn status(label: &str, value: &str) {
    eprintln!("  {} {}", ansi("1", &format!("{label:<20}")), value);
}

/// Print a header (legacy — prefer banner)
pub fn header(msg: &str) {
    eprintln!("\n{}\n", ansi("1;4", msg));
}

// ---------------------------------------------------------------------------
// Spinner
// ---------------------------------------------------------------------------

const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

// ---------------------------------------------------------------------------
// DeployProgress — resource progress tracking during step loop
// ---------------------------------------------------------------------------

struct DeployProgressState {
    status: Option<DeploymentStatus>,
    resources: BTreeMap<String, (String, ResourceStatus)>,
    rendered_line_count: usize,
    tick: usize,
}

pub struct DeployProgress {
    is_tty: bool,
    inner: Arc<Mutex<DeployProgressState>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    /// Previous state for log-mode (non-TTY) delta output
    last_log_status: Option<DeploymentStatus>,
    last_log_resources: BTreeMap<String, (String, ResourceStatus)>,
}

impl DeployProgress {
    pub fn new() -> Self {
        let is_tty = is_tty();
        let inner = Arc::new(Mutex::new(DeployProgressState {
            status: None,
            resources: BTreeMap::new(),
            rendered_line_count: 0,
            tick: 0,
        }));
        let stop = Arc::new(AtomicBool::new(false));

        let thread = if is_tty {
            let thread_inner = Arc::clone(&inner);
            let thread_stop = Arc::clone(&stop);
            Some(std::thread::spawn(move || {
                while !thread_stop.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(120));
                    if thread_stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let mut state = thread_inner.lock().unwrap_or_else(|e| e.into_inner());
                    if state.status.is_some() {
                        state.tick += 1;
                        render_tty(&mut state);
                    }
                }
            }))
        } else {
            None
        };

        Self {
            is_tty,
            inner,
            stop,
            thread,
            last_log_status: None,
            last_log_resources: BTreeMap::new(),
        }
    }

    pub fn update(&mut self, progress: &StepProgress) {
        let resources = progress
            .stack_state
            .map(|ss| extract_resources(ss))
            .unwrap_or_default();

        if self.is_tty {
            let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            state.status = Some(progress.status);
            state.resources = resources;
            render_tty(&mut state);
        } else {
            self.render_log(progress.status, &resources);
            self.last_log_status = Some(progress.status);
            self.last_log_resources = resources;
        }
    }

    pub fn finish(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        if self.is_tty {
            let state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if state.rendered_line_count > 0 {
                // Clear the live spinner output, then render a static final snapshot.
                eprint!("\x1b[{}A\x1b[J", state.rendered_line_count);
            }
            render_final(&state);
        }
    }

    fn render_log(
        &self,
        status: DeploymentStatus,
        resources: &BTreeMap<String, (String, ResourceStatus)>,
    ) {
        // Print deployment status changes
        if self.last_log_status.as_ref() != Some(&status) {
            eprintln!(
                "Deployment status: {}",
                format_deployment_status(status)
            );
        }

        // Print resource status changes
        for (name, (resource_type, res_status)) in resources {
            let prev = self.last_log_resources.get(name);
            if prev.is_none() || prev.map(|(_, s)| s) != Some(res_status) {
                eprintln!(
                    "  {} ({}): {}",
                    name,
                    resource_type,
                    format_resource_status(*res_status)
                );
            }
        }
    }
}

impl Drop for DeployProgress {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn render_tty(state: &mut DeployProgressState) {
    let Some(status) = state.status else {
        return;
    };

    let frame = SPINNER_FRAMES[state.tick % SPINNER_FRAMES.len()];

    // Clear previous output
    let mut buf = String::new();
    if state.rendered_line_count > 0 {
        buf.push_str(&format!("\x1b[{}A\x1b[J", state.rendered_line_count));
    }

    // Hide cursor during render to prevent flicker
    buf.push_str("\x1b[?25l");

    let mut line_count = 0;

    // Status line
    let status_label = format_deployment_status(status);
    buf.push_str(&format!(
        "\x1b[36m{}\x1b[0m \x1b[1mDeploying... ({})\x1b[0m\n",
        frame, status_label
    ));
    line_count += 1;

    // Resource lines
    if !state.resources.is_empty() {
        let max_name_len = state
            .resources
            .keys()
            .map(|k| k.len())
            .max()
            .unwrap_or(0)
            .max(8);

        for (name, (resource_type, res_status)) in &state.resources {
            let prefix = match res_status {
                ResourceStatus::Running => "  \x1b[32m✔\x1b[0m".to_string(),
                ResourceStatus::Pending => format!("  \x1b[2m·\x1b[0m"),
                ResourceStatus::ProvisionFailed
                | ResourceStatus::UpdateFailed
                | ResourceStatus::DeleteFailed
                | ResourceStatus::RefreshFailed => "  \x1b[31m✗\x1b[0m".to_string(),
                _ => format!("  \x1b[36m{}\x1b[0m", frame),
            };
            let status_str = format_resource_status(*res_status);
            let styled_status = match res_status {
                ResourceStatus::Running => format!("\x1b[32m{}\x1b[0m", status_str),
                ResourceStatus::ProvisionFailed
                | ResourceStatus::UpdateFailed
                | ResourceStatus::DeleteFailed
                | ResourceStatus::RefreshFailed => format!("\x1b[31m{}\x1b[0m", status_str),
                ResourceStatus::Pending => format!("\x1b[2m{}\x1b[0m", status_str),
                _ => format!("\x1b[33m{}\x1b[0m", status_str),
            };

            buf.push_str(&format!(
                "{} {:<width$}  \x1b[2m({})\x1b[0m  {}\n",
                prefix,
                name,
                resource_type,
                styled_status,
                width = max_name_len,
            ));
            line_count += 1;
        }
    }

    // Show cursor again
    buf.push_str("\x1b[?25h");

    state.rendered_line_count = line_count;

    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(buf.as_bytes());
    let _ = stderr.flush();
}

fn render_final(state: &DeployProgressState) {
    if state.resources.is_empty() {
        return;
    }

    let mut buf = String::new();
    let max_name_len = state
        .resources
        .keys()
        .map(|k| k.len())
        .max()
        .unwrap_or(0)
        .max(8);

    for (name, (resource_type, res_status)) in &state.resources {
        let prefix = match res_status {
            ResourceStatus::Running => "  \x1b[32m✔\x1b[0m",
            ResourceStatus::ProvisionFailed
            | ResourceStatus::UpdateFailed
            | ResourceStatus::DeleteFailed
            | ResourceStatus::RefreshFailed => "  \x1b[31m✗\x1b[0m",
            _ => "  \x1b[33m·\x1b[0m",
        };
        let status_str = format_resource_status(*res_status);
        let styled_status = match res_status {
            ResourceStatus::Running => format!("\x1b[32m{}\x1b[0m", status_str),
            ResourceStatus::ProvisionFailed
            | ResourceStatus::UpdateFailed
            | ResourceStatus::DeleteFailed
            | ResourceStatus::RefreshFailed => format!("\x1b[31m{}\x1b[0m", status_str),
            _ => format!("\x1b[33m{}\x1b[0m", status_str),
        };

        buf.push_str(&format!(
            "{} {:<width$}  \x1b[2m({})\x1b[0m  {}\n",
            prefix,
            name,
            resource_type,
            styled_status,
            width = max_name_len,
        ));
    }

    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(buf.as_bytes());
    let _ = stderr.flush();
}

fn extract_resources(
    stack_state: &StackState,
) -> BTreeMap<String, (String, ResourceStatus)> {
    stack_state
        .resources
        .iter()
        .map(|(name, res)| {
            (
                name.clone(),
                (res.resource_type.clone(), res.status),
            )
        })
        .collect()
}

fn format_deployment_status(status: DeploymentStatus) -> &'static str {
    match status {
        DeploymentStatus::Pending => "pending",
        DeploymentStatus::InitialSetup => "initializing",
        DeploymentStatus::InitialSetupFailed => "setup failed",
        DeploymentStatus::Provisioning => "provisioning",
        DeploymentStatus::ProvisioningFailed => "provisioning failed",
        DeploymentStatus::Running => "running",
        DeploymentStatus::RefreshFailed => "refresh failed",
        DeploymentStatus::UpdatePending => "update pending",
        DeploymentStatus::Updating => "updating",
        DeploymentStatus::UpdateFailed => "update failed",
        DeploymentStatus::DeletePending => "delete pending",
        DeploymentStatus::Deleting => "deleting",
        DeploymentStatus::DeleteFailed => "delete failed",
        DeploymentStatus::Deleted => "deleted",
        DeploymentStatus::Error => "error",
    }
}

fn format_resource_status(status: ResourceStatus) -> &'static str {
    match status {
        ResourceStatus::Pending => "pending",
        ResourceStatus::Provisioning => "provisioning",
        ResourceStatus::ProvisionFailed => "provision failed",
        ResourceStatus::Running => "running",
        ResourceStatus::Updating => "updating",
        ResourceStatus::UpdateFailed => "update failed",
        ResourceStatus::Deleting => "deleting",
        ResourceStatus::DeleteFailed => "delete failed",
        ResourceStatus::Deleted => "deleted",
        ResourceStatus::RefreshFailed => "refresh failed",
    }
}
