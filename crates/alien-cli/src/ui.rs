use std::collections::HashMap;
use std::io::{self, IsTerminal};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use alien_core::{
    AlienEvent, DeploymentStatus, EventBus, EventChange, EventHandler, EventState, Platform,
    PushProgress, ResourceStatus, StackResourceState,
};
use alien_error::{AlienError, AlienErrorData};
use async_trait::async_trait;
use comfy_table::{
    modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL_CONDENSED, Attribute, Cell, Color,
    ContentArrangement, Table,
};
use console::{measure_text_width, style, Term};
use indexmap::IndexMap;

// ---------------------------------------------------------------------------
// Shared utilities
// ---------------------------------------------------------------------------

pub fn supports_ansi() -> bool {
    io::stdout().is_terminal() && io::stderr().is_terminal()
}

pub fn heading(message: &str) -> String {
    if supports_ansi() {
        style(message).bold().cyan().to_string()
    } else {
        message.to_string()
    }
}

pub fn highlighted_value(value: &str) -> String {
    if supports_ansi() {
        style(value).bold().cyan().to_string()
    } else {
        value.to_string()
    }
}

pub fn contextual_heading(action: &str, subject: &str, contexts: &[(&str, &str)]) -> String {
    let action = if supports_ansi() {
        style(action).bold().to_string()
    } else {
        action.to_string()
    };

    let mut rendered = format!("{action} {}", highlighted_value(subject));
    for (connector, value) in contexts {
        rendered.push(' ');
        rendered.push_str(connector);
        rendered.push(' ');
        rendered.push_str(&highlighted_value(value));
    }

    rendered
}

pub fn print_cli_banner(current_dir: &Path) {
    let cwd = abbreviate_home(current_dir);
    let glyph = ["  .-^-.", " (o u o)", "  \\_=_/"];
    let title = if supports_ansi() {
        style(format!("Alien CLI v{}", env!("CARGO_PKG_VERSION")))
            .bold()
            .to_string()
    } else {
        format!("Alien CLI v{}", env!("CARGO_PKG_VERSION"))
    };
    let rows = [
        (glyph[0], title),
        (glyph[1], dim_label("Ship into your customer's cloud")),
        (glyph[2], dim_label(&cwd)),
    ];

    for (raw_glyph, text) in rows {
        let padded_glyph = format!("{raw_glyph:<8}");
        let rendered_glyph = if supports_ansi() {
            style(padded_glyph).green().bold().to_string()
        } else {
            padded_glyph
        };
        println!("{rendered_glyph} {text}");
    }
    println!();
}

pub fn make_table(headers: &[&str]) -> Table {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.apply_modifier(UTF8_ROUND_CORNERS);
    table.set_content_arrangement(ContentArrangement::Dynamic);
    let header_cells: Vec<Cell> = headers
        .iter()
        .map(|header| {
            let cell = Cell::new(*header).add_attribute(Attribute::Bold);
            if supports_ansi() {
                cell.fg(Color::Cyan)
            } else {
                cell
            }
        })
        .collect();
    table.set_header(header_cells);
    table
}

pub fn print_table(table: Table) {
    println!("{table}");
}

pub fn status_cell(status: &str) -> Cell {
    let normalized = status.to_ascii_lowercase();
    let cell = Cell::new(status);
    if !supports_ansi() {
        return cell;
    }

    if normalized.contains("running")
        || normalized.contains("ready")
        || normalized.contains("success")
        || normalized.contains("active")
    {
        cell.fg(Color::Green)
    } else if normalized.contains("failed") || normalized.contains("error") {
        cell.fg(Color::Red)
    } else if normalized.contains("pending")
        || normalized.contains("queued")
        || normalized.contains("provision")
        || normalized.contains("updating")
        || normalized.contains("deleting")
    {
        cell.fg(Color::Yellow)
    } else {
        cell.fg(Color::Cyan)
    }
}

fn abbreviate_home(path: &Path) -> String {
    let display = path.display().to_string();
    let home = std::env::var("HOME").ok();

    match home {
        Some(home) if display.starts_with(&home) => format!("~{}", &display[home.len()..]),
        _ => display,
    }
}

pub fn success_line(message: &str) -> String {
    if supports_ansi() {
        format!("{} {}", style("> Success!").cyan(), message)
    } else {
        format!("> Success! {message}")
    }
}

pub fn dim_label(label: &str) -> String {
    if supports_ansi() {
        style(label).dim().to_string()
    } else {
        label.to_string()
    }
}

pub fn accent(value: &str) -> String {
    if supports_ansi() {
        style(value).cyan().to_string()
    } else {
        value.to_string()
    }
}

pub fn command(value: &str) -> String {
    if supports_ansi() {
        style(value).bold().to_string()
    } else {
        value.to_string()
    }
}

pub fn render_human_error<T>(error: &AlienError<T>) -> String
where
    T: AlienErrorData + Clone + std::fmt::Debug + serde::Serialize,
{
    let report = error.human_report();
    let mut rendered = if supports_ansi() {
        style(report.message).red().to_string()
    } else {
        report.message
    };

    if !report.causes.is_empty() {
        rendered.push('\n');
        rendered.push_str("Cause:");
        for cause in report.causes {
            rendered.push('\n');
            rendered.push_str(&format!("  - {}", cause.message));
        }
    }

    if let Some(hint) = report.hint {
        rendered.push('\n');
        rendered.push_str("Next:");
        rendered.push('\n');
        rendered.push_str(&format!("  {hint}"));
    }

    rendered
}

// ---------------------------------------------------------------------------
// LiveRegion — the core terminal renderer that replaces indicatif
// ---------------------------------------------------------------------------

const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const SPINNER_PLACEHOLDER: &str = "{spinner}";

struct LiveRegionInner {
    term: Term,
    sections: IndexMap<String, Vec<String>>,
    rendered_count: usize,
    tick: usize,
}

impl LiveRegionInner {
    /// Clear the current live region from the terminal and write new content.
    fn render(&mut self) {
        // Collect all lines from all sections
        let lines: Vec<&str> = self
            .sections
            .values()
            .flat_map(|v| v.iter().map(String::as_str))
            .collect();

        // Clear previous output
        if self.rendered_count > 0 {
            let _ = self.term.clear_last_lines(self.rendered_count);
        }

        // Write new lines with spinner substitution
        let frame = SPINNER_FRAMES[self.tick % SPINNER_FRAMES.len()];
        let styled_frame = if supports_ansi() {
            style(frame).cyan().to_string()
        } else {
            frame.to_string()
        };

        for line in &lines {
            let output = line.replace(SPINNER_PLACEHOLDER, &styled_frame);
            let _ = self.term.write_line(&output);
        }
        let _ = self.term.flush();
        self.rendered_count = lines.len();
    }
}

pub struct LiveRegion {
    inner: Arc<Mutex<LiveRegionInner>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl LiveRegion {
    pub fn new() -> Self {
        let inner = Arc::new(Mutex::new(LiveRegionInner {
            term: Term::stderr(),
            sections: IndexMap::new(),
            rendered_count: 0,
            tick: 0,
        }));

        let stop = Arc::new(AtomicBool::new(false));

        // Spawn animation thread that re-renders every 120ms
        let thread_inner = Arc::clone(&inner);
        let thread_stop = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(120));
                if thread_stop.load(Ordering::Relaxed) {
                    break;
                }
                let mut guard = thread_inner.lock().unwrap_or_else(|e| e.into_inner());
                guard.tick += 1;
                // Only re-render if there's content with spinners, or if dirty
                if !guard.sections.is_empty() {
                    guard.render();
                }
            }
        });

        Self {
            inner,
            stop,
            thread: Some(thread),
        }
    }

    /// Update a named section's content. Sections are rendered in insertion order.
    pub fn set_section(&self, name: &str, lines: Vec<String>) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if lines.is_empty() {
            guard.sections.shift_remove(name);
        } else {
            // Use entry API to preserve insertion order for existing keys
            match guard.sections.entry(name.to_string()) {
                indexmap::map::Entry::Occupied(mut e) => {
                    *e.get_mut() = lines;
                }
                indexmap::map::Entry::Vacant(e) => {
                    e.insert(lines);
                }
            }
        }
        guard.render();
    }

    /// Print a permanent line above the live region.
    pub fn println(&self, line: &str) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        // Clear live region
        if guard.rendered_count > 0 {
            let _ = guard.term.clear_last_lines(guard.rendered_count);
            guard.rendered_count = 0;
        }
        // Write permanent line
        let _ = guard.term.write_line(line);
        // Re-render live region below
        guard.render();
    }

    /// Clear all rendered content from the terminal.
    pub fn clear(&self) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if guard.rendered_count > 0 {
            let _ = guard.term.clear_last_lines(guard.rendered_count);
            guard.rendered_count = 0;
        }
        guard.sections.clear();
    }
}

impl Drop for LiveRegion {
    fn drop(&mut self) {
        // Signal the animation thread to stop
        self.stop.store(true, Ordering::Relaxed);
        // Wait for it to finish (max ~120ms)
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        // Do NOT clear — leave the last rendered content visible on screen.
        // This is the entire point: no duplicate rendering on drop.
    }
}

// ---------------------------------------------------------------------------
// RowState — shared enum for step/resource states
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    Pending,
    Active,
    Complete,
    Failed,
    Skipped,
}

fn prefix_for_state(state: RowState) -> String {
    match state {
        RowState::Pending => dim_label("·"),
        RowState::Active => active_prefix(),
        RowState::Complete => success_prefix(),
        RowState::Failed => error_prefix(),
        RowState::Skipped => dim_label("-"),
    }
}

fn active_prefix() -> String {
    if supports_ansi() {
        style(">").cyan().bold().to_string()
    } else {
        ">".to_string()
    }
}

fn success_prefix() -> String {
    if supports_ansi() {
        style("✔").cyan().to_string()
    } else {
        "✔".to_string()
    }
}

fn error_prefix() -> String {
    if supports_ansi() {
        style("✘").red().to_string()
    } else {
        "✘".to_string()
    }
}

fn format_message(label: &str, detail: Option<&str>) -> String {
    match detail {
        Some(detail) if !detail.is_empty() => format!("{label} {detail}"),
        _ => label.to_string(),
    }
}

/// Build a single display line for a step or resource row.
fn build_row_line(state: RowState, label: &str, detail: Option<&str>) -> String {
    let prefix = match state {
        RowState::Active => SPINNER_PLACEHOLDER.to_string(),
        _ => prefix_for_state(state),
    };
    let msg = format_message(label, detail);
    format!("{prefix} {msg}")
}

/// Build a text progress bar like `[=====>       ] 45%`
fn build_progress_bar_text(position: u64, total: u64, width: usize) -> String {
    if total == 0 {
        return format!("[{}]", " ".repeat(width));
    }
    let ratio = (position as f64 / total as f64).min(1.0);
    let filled = (ratio * width as f64) as usize;
    let empty = width.saturating_sub(filled).saturating_sub(1);

    if supports_ansi() {
        let bar_content = if filled >= width {
            style("=".repeat(width)).cyan().to_string()
        } else {
            format!(
                "{}{}{}",
                style("=".repeat(filled)).cyan(),
                style(">").cyan(),
                " ".repeat(empty)
            )
        };
        format!("[{bar_content}]")
    } else if filled >= width {
        format!("[{}]", "=".repeat(width))
    } else {
        format!("[{}{}>{}]", "=".repeat(filled), "", " ".repeat(empty))
    }
}

fn build_resource_progress_line(
    label: &str,
    detail: Option<&str>,
    progress: &PushProgress,
) -> String {
    let msg = format_message(label, detail);

    // When totals are not yet known, show only the spinner + message
    // (no empty bar that looks stuck at 0%).
    if progress.total_bytes == 0 && progress.total_layers == 0 {
        return format!("{SPINNER_PLACEHOLDER} {msg}");
    }

    let bar = build_progress_bar_text(
        if progress.total_bytes > 0 {
            progress.bytes_uploaded
        } else {
            progress.layers_uploaded as u64
        },
        if progress.total_bytes > 0 {
            progress.total_bytes
        } else {
            progress.total_layers as u64
        },
        24,
    );
    let suffix = if progress.total_bytes > 0 {
        format!(
            "{}/{}",
            format_bytes(progress.bytes_uploaded),
            format_bytes(progress.total_bytes)
        )
    } else if progress.total_layers > 0 {
        format!("{}/{}", progress.layers_uploaded, progress.total_layers)
    } else {
        String::new()
    };
    format!("{SPINNER_PLACEHOLDER} {msg} {bar} {suffix}")
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1}GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1}MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes}B")
    }
}

// ---------------------------------------------------------------------------
// FixedSteps — multi-step progress tracker
// ---------------------------------------------------------------------------

struct FixedStepsResource {
    label: String,
    state: RowState,
    detail: Option<String>,
    progress: Option<PushProgress>,
}

struct FixedStepsState {
    step_labels: Vec<String>,
    step_states: Vec<RowState>,
    step_details: Vec<Option<String>>,
    resources: IndexMap<String, FixedStepsResource>,
}

impl FixedStepsState {
    fn build_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();

        for i in 0..self.step_labels.len() {
            let state = self.step_states[i];
            if matches!(state, RowState::Pending) {
                // Don't show pending steps — only show when activated
                continue;
            }
            lines.push(build_row_line(
                state,
                &self.step_labels[i],
                self.step_details[i].as_deref(),
            ));
        }

        for resource in self.resources.values() {
            if let Some(progress) = &resource.progress {
                lines.push(build_resource_progress_line(
                    &resource.label,
                    resource.detail.as_deref(),
                    progress,
                ));
            } else {
                lines.push(build_row_line(
                    resource.state,
                    &resource.label,
                    resource.detail.as_deref(),
                ));
            }
        }

        lines
    }
}

#[derive(Clone)]
pub struct FixedSteps {
    live: Option<Arc<LiveRegion>>,
    state: Option<Arc<Mutex<FixedStepsState>>>,
}

impl FixedSteps {
    pub fn new(step_labels: &[&str]) -> Self {
        if !supports_ansi() {
            return Self {
                live: None,
                state: None,
            };
        }

        Self {
            live: Some(Arc::new(LiveRegion::new())),
            state: Some(Arc::new(Mutex::new(FixedStepsState {
                step_labels: step_labels.iter().map(|l| (*l).to_string()).collect(),
                step_states: vec![RowState::Pending; step_labels.len()],
                step_details: vec![None; step_labels.len()],
                resources: IndexMap::new(),
            }))),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.live.is_some()
    }

    pub fn live_region(&self) -> Option<Arc<LiveRegion>> {
        self.live.clone()
    }

    fn update_live(&self) {
        if let (Some(live), Some(state)) = (&self.live, &self.state) {
            let guard = state.lock().unwrap_or_else(|e| e.into_inner());
            live.set_section("steps", guard.build_lines());
        }
    }

    pub fn activate(&self, index: usize, detail: Option<impl Into<String>>) {
        if let Some(state) = &self.state {
            let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());
            if index < guard.step_states.len() {
                guard.step_states[index] = RowState::Active;
                guard.step_details[index] = detail.map(Into::into);
            }
            drop(guard);
            self.update_live();
        }
    }

    pub fn complete(&self, index: usize, detail: Option<impl Into<String>>) {
        if let Some(state) = &self.state {
            let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());
            if index < guard.step_states.len() {
                guard.step_states[index] = RowState::Complete;
                guard.step_details[index] = detail.map(Into::into);
            }
            drop(guard);
            self.update_live();
        }
    }

    pub fn fail(&self, index: usize, detail: Option<impl Into<String>>) {
        if let Some(state) = &self.state {
            let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());
            if index < guard.step_states.len() {
                guard.step_states[index] = RowState::Failed;
                guard.step_details[index] = detail.map(Into::into);
            }
            drop(guard);
            self.update_live();
        }
    }

    pub fn skip(&self, index: usize, detail: Option<impl Into<String>>) {
        if let Some(state) = &self.state {
            let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());
            if index < guard.step_states.len() {
                guard.step_states[index] = RowState::Skipped;
                guard.step_details[index] = detail.map(Into::into);
            }
            drop(guard);
            self.update_live();
        }
    }

    pub fn resource_active(&self, key: &str, label: impl Into<String>, detail: Option<String>) {
        if let Some(state) = &self.state {
            let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());
            let entry =
                guard
                    .resources
                    .entry(key.to_string())
                    .or_insert_with(|| FixedStepsResource {
                        label: String::new(),
                        state: RowState::Pending,
                        detail: None,
                        progress: None,
                    });
            entry.label = label.into();
            entry.state = RowState::Active;
            entry.detail = detail;
            entry.progress = None;
            drop(guard);
            self.update_live();
        }
    }

    pub fn resource_complete(&self, key: &str, detail: Option<String>) {
        if let Some(state) = &self.state {
            let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(entry) = guard.resources.get_mut(key) {
                entry.state = RowState::Complete;
                entry.detail = detail;
                entry.progress = None;
            }
            drop(guard);
            self.update_live();
        }
    }

    pub fn resource_fail(&self, key: &str, detail: Option<String>) {
        if let Some(state) = &self.state {
            let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(entry) = guard.resources.get_mut(key) {
                entry.state = RowState::Failed;
                entry.detail = detail;
                entry.progress = None;
            }
            drop(guard);
            self.update_live();
        }
    }

    pub fn resource_progress(
        &self,
        key: &str,
        label: impl Into<String>,
        detail: Option<String>,
        progress: &PushProgress,
    ) {
        if let Some(state) = &self.state {
            let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());
            let entry =
                guard
                    .resources
                    .entry(key.to_string())
                    .or_insert_with(|| FixedStepsResource {
                        label: String::new(),
                        state: RowState::Active,
                        detail: None,
                        progress: None,
                    });
            entry.label = label.into();
            entry.state = RowState::Active;
            entry.detail = detail;
            entry.progress = Some(progress.clone());
            drop(guard);
            self.update_live();
        }
    }

    pub fn println(&self, line: &str) {
        if let Some(live) = &self.live {
            live.println(line);
        } else {
            println!("{line}");
        }
    }

    pub fn sync_deployment_resources(
        &self,
        resources: &std::collections::HashMap<String, StackResourceState>,
    ) {
        if let Some(state) = &self.state {
            let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());

            let mut entries: Vec<_> = resources.iter().collect();
            entries.sort_by(|(a, _), (b, _)| a.cmp(b));

            for (resource_name, resource) in entries {
                let key = format!("deployment:{resource_name}");
                let label = deployment_resource_label(resource_name, resource);
                let detail = deployment_resource_detail(resource);
                let row_state = match resource.status {
                    ResourceStatus::Running | ResourceStatus::Deleted => RowState::Complete,
                    ResourceStatus::ProvisionFailed
                    | ResourceStatus::UpdateFailed
                    | ResourceStatus::DeleteFailed
                    | ResourceStatus::RefreshFailed => RowState::Failed,
                    ResourceStatus::Pending
                    | ResourceStatus::Provisioning
                    | ResourceStatus::Updating
                    | ResourceStatus::Deleting => RowState::Active,
                };

                let entry = guard
                    .resources
                    .entry(key)
                    .or_insert_with(|| FixedStepsResource {
                        label: String::new(),
                        state: RowState::Pending,
                        detail: None,
                        progress: None,
                    });
                entry.label = label;
                entry.state = row_state;
                entry.detail = Some(
                    detail.unwrap_or_else(|| format_resource_status(resource.status).to_string()),
                );
                entry.progress = None;
            }

            drop(guard);
            self.update_live();
        }
    }
}

// ---------------------------------------------------------------------------
// DevCardScreen — card-based deployment display
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DevResourceEntry {
    pub name: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DevDeploymentCard {
    pub name: String,
    pub status: DeploymentStatus,
    pub platform: Option<Platform>,
    pub resources: Vec<DevResourceEntry>,
    pub error: Option<String>,
}

pub struct DevCardScreen {
    live: Option<Arc<LiveRegion>>,
}

impl DevCardScreen {
    pub fn new(parent: Option<Arc<LiveRegion>>) -> Self {
        let live = match parent {
            Some(lr) => Some(lr),
            None if supports_ansi() => Some(Arc::new(LiveRegion::new())),
            None => None,
        };

        Self { live }
    }

    pub fn is_enabled(&self) -> bool {
        self.live.is_some()
    }

    pub fn update(&self, cards: &[DevDeploymentCard]) {
        let Some(live) = &self.live else {
            return;
        };

        let cards_text = render_deployment_cards(cards);
        let footer_text = render_dev_actions_footer();
        let mut lines: Vec<String> = Vec::new();
        for line in cards_text.lines() {
            lines.push(line.to_string());
        }
        for line in footer_text.lines() {
            lines.push(line.to_string());
        }
        live.set_section("cards", lines);
    }

    pub fn println(&self, line: &str) {
        if let Some(live) = &self.live {
            live.println(line);
        } else {
            println!("{line}");
        }
    }
}

// No Drop impl — LiveRegion leaves the last rendered content visible on drop.
// Clearing the section here would wipe the cards before the user sees them.

// ---------------------------------------------------------------------------
// Status formatting
// ---------------------------------------------------------------------------

pub fn format_resource_status(status: ResourceStatus) -> &'static str {
    match status {
        ResourceStatus::Pending => "Queued",
        ResourceStatus::Provisioning => "Provisioning",
        ResourceStatus::ProvisionFailed => "Failed",
        ResourceStatus::Running => "Ready",
        ResourceStatus::Updating => "Updating",
        ResourceStatus::UpdateFailed => "Failed",
        ResourceStatus::Deleting => "Deleting",
        ResourceStatus::DeleteFailed => "Failed",
        ResourceStatus::Deleted => "Deleted",
        ResourceStatus::RefreshFailed => "Failed",
    }
}

pub fn format_deployment_status(status: DeploymentStatus) -> &'static str {
    match status {
        DeploymentStatus::Pending => "Queued",
        DeploymentStatus::InitialSetup => "Initializing",
        DeploymentStatus::InitialSetupFailed => "Failed",
        DeploymentStatus::Provisioning => "Provisioning",
        DeploymentStatus::ProvisioningFailed => "Failed",
        DeploymentStatus::Running => "Running",
        DeploymentStatus::RefreshFailed => "Failed",
        DeploymentStatus::UpdatePending => "Queued",
        DeploymentStatus::Updating => "Updating",
        DeploymentStatus::UpdateFailed => "Failed",
        DeploymentStatus::DeletePending => "Queued",
        DeploymentStatus::Deleting => "Deleting",
        DeploymentStatus::DeleteFailed => "Failed",
        DeploymentStatus::Deleted => "Deleted",
        DeploymentStatus::Error => "Error",
    }
}

pub fn deployment_resource_detail(resource: &StackResourceState) -> Option<String> {
    match resource.status {
        ResourceStatus::ProvisionFailed
        | ResourceStatus::UpdateFailed
        | ResourceStatus::DeleteFailed
        | ResourceStatus::RefreshFailed => {
            resource.error.as_ref().map(|error| error.message.clone())
        }
        _ => None,
    }
}

fn deployment_resource_label(resource_name: &str, resource: &StackResourceState) -> String {
    format!("{} ({})", resource_name, resource.resource_type)
}

// ---------------------------------------------------------------------------
// UiCommandKind + EventBus integration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiCommandKind {
    Build,
    Release,
}

pub fn event_bus_for_command(kind: Option<UiCommandKind>, json_output: bool) -> Option<EventBus> {
    if json_output || !supports_ansi() {
        return None;
    }

    let kind = kind?;
    let handler = Arc::new(CommandEventHandler::new(kind));
    Some(EventBus::with_handlers(vec![handler]))
}

// ---------------------------------------------------------------------------
// CommandEventHandler — event-driven progress for build/release
// ---------------------------------------------------------------------------

struct CommandEventHandler {
    kind: UiCommandKind,
    live: LiveRegion,
    state: Mutex<CommandEventState>,
}

struct CommandEventState {
    status_message: Option<String>,
    event_roles: HashMap<String, EventRole>,
    resources: IndexMap<String, CommandResource>,
}

#[derive(Clone)]
enum EventRole {
    Status,
    ResourceScope(String),
    ResourceChild(String),
}

struct CommandResource {
    label: String,
    state: RowState,
    detail: Option<String>,
    progress: Option<PushProgress>,
}

impl CommandEventHandler {
    fn new(kind: UiCommandKind) -> Self {
        Self {
            kind,
            live: LiveRegion::new(),
            state: Mutex::new(CommandEventState {
                status_message: None,
                event_roles: HashMap::new(),
                resources: IndexMap::new(),
            }),
        }
    }

    fn rebuild_lines(state: &CommandEventState) -> Vec<String> {
        let mut lines = Vec::new();

        if let Some(msg) = &state.status_message {
            lines.push(format!("{SPINNER_PLACEHOLDER} {msg}"));
        }

        for resource in state.resources.values() {
            if let Some(progress) = &resource.progress {
                lines.push(build_resource_progress_line(
                    &resource.label,
                    resource.detail.as_deref(),
                    progress,
                ));
            } else {
                lines.push(build_row_line(
                    resource.state,
                    &resource.label,
                    resource.detail.as_deref(),
                ));
            }
        }

        lines
    }

    fn set_status(&self, state: &mut CommandEventState, message: impl Into<String>) {
        state.status_message = Some(message.into());
    }

    fn clear_status(&self, state: &mut CommandEventState) {
        state.status_message = None;
    }

    fn handle_build_created(
        &self,
        state: &mut CommandEventState,
        id: &str,
        parent_id: Option<&str>,
        event: &AlienEvent,
    ) {
        match event {
            AlienEvent::LoadingConfiguration => {
                self.set_status(state, "Loading configuration");
                state.event_roles.insert(id.to_string(), EventRole::Status);
            }
            AlienEvent::RunningPreflights { stack, platform } => {
                self.set_status(
                    state,
                    format!("Running preflights for {stack} ({platform})"),
                );
                state.event_roles.insert(id.to_string(), EventRole::Status);
            }
            AlienEvent::BuildingResource {
                resource_name,
                resource_type,
                related_resources,
            } => {
                self.clear_status(state);
                let key = format!("build:{resource_type}:{resource_name}");
                let label = build_resource_noun(resource_name, resource_type, related_resources);
                let entry = state
                    .resources
                    .entry(key.clone())
                    .or_insert_with(|| CommandResource {
                        label: label.clone(),
                        state: RowState::Active,
                        detail: None,
                        progress: None,
                    });
                entry.label = label;
                entry.state = RowState::Active;
                entry.detail = None;
                state
                    .event_roles
                    .insert(id.to_string(), EventRole::ResourceScope(key));
            }
            AlienEvent::CompilingCode { language, progress } => {
                if let Some(resource_key) =
                    parent_id.and_then(|parent| resource_key_from_parent_or_child(state, parent))
                {
                    state.event_roles.insert(
                        id.to_string(),
                        EventRole::ResourceChild(resource_key.clone()),
                    );
                    if let Some(entry) = state.resources.get_mut(&resource_key) {
                        entry.detail = progress
                            .clone()
                            .or_else(|| Some(format!("compiling {language}")));
                    }
                }
            }
            AlienEvent::Finished => {
                self.clear_status(state);
            }
            _ => {}
        }
    }

    fn handle_release_created(
        &self,
        state: &mut CommandEventState,
        id: &str,
        parent_id: Option<&str>,
        event: &AlienEvent,
    ) {
        match event {
            AlienEvent::LoadingConfiguration => {
                self.set_status(state, "Loading configuration");
                state.event_roles.insert(id.to_string(), EventRole::Status);
            }
            AlienEvent::RunningPreflights { stack, platform } => {
                self.set_status(
                    state,
                    format!("Running preflights for {stack} ({platform})"),
                );
                state.event_roles.insert(id.to_string(), EventRole::Status);
            }
            AlienEvent::BuildingResource {
                resource_name,
                resource_type,
                related_resources,
            } => {
                self.clear_status(state);
                let key = format!("release-build:{resource_type}:{resource_name}");
                let label = build_resource_noun(resource_name, resource_type, related_resources);
                let entry = state
                    .resources
                    .entry(key.clone())
                    .or_insert_with(|| CommandResource {
                        label: label.clone(),
                        state: RowState::Active,
                        detail: None,
                        progress: None,
                    });
                entry.label = label;
                entry.state = RowState::Active;
                entry.detail = None;
                state
                    .event_roles
                    .insert(id.to_string(), EventRole::ResourceScope(key));
            }
            AlienEvent::CompilingCode { language, progress } => {
                if let Some(resource_key) =
                    parent_id.and_then(|parent| resource_key_from_parent_or_child(state, parent))
                {
                    state.event_roles.insert(
                        id.to_string(),
                        EventRole::ResourceChild(resource_key.clone()),
                    );
                    if let Some(entry) = state.resources.get_mut(&resource_key) {
                        entry.detail = progress
                            .clone()
                            .or_else(|| Some(format!("compiling {language}")));
                    }
                }
            }
            AlienEvent::PushingResource {
                resource_name,
                resource_type,
            } => {
                self.clear_status(state);
                let key = format!("push:{resource_type}:{resource_name}");
                let label = format!("{resource_type} {resource_name}");
                let entry = state
                    .resources
                    .entry(key.clone())
                    .or_insert_with(|| CommandResource {
                        label: label.clone(),
                        state: RowState::Active,
                        detail: None,
                        progress: None,
                    });
                entry.label = label;
                entry.state = RowState::Active;
                entry.detail = Some("preparing".to_string());
                state
                    .event_roles
                    .insert(id.to_string(), EventRole::ResourceScope(key));
            }
            AlienEvent::PushingImage { image, progress } => {
                if let Some(resource_key) =
                    parent_id.and_then(|parent| resource_key_from_parent_or_child(state, parent))
                {
                    state.event_roles.insert(
                        id.to_string(),
                        EventRole::ResourceChild(resource_key.clone()),
                    );
                    if let Some(entry) = state.resources.get_mut(&resource_key) {
                        entry.detail = progress
                            .as_ref()
                            .map(|p| p.operation.clone())
                            .or_else(|| Some(format!("pushing {image}")));
                        entry.progress = progress.clone();
                    }
                }
            }
            AlienEvent::CreatingRelease { project } => {
                self.set_status(state, format!("Creating release for {project}"));
                state.event_roles.insert(id.to_string(), EventRole::Status);
            }
            AlienEvent::Finished => {
                self.clear_status(state);
            }
            _ => {}
        }
    }
}

#[async_trait]
impl EventHandler for CommandEventHandler {
    async fn on_event_change(&self, change: EventChange) -> alien_core::Result<()> {
        match change {
            EventChange::Created {
                id,
                parent_id,
                event,
                ..
            } => {
                let mut state = self.state.lock().expect("command event state poisoned");
                match self.kind {
                    UiCommandKind::Build => {
                        self.handle_build_created(&mut state, &id, parent_id.as_deref(), &event)
                    }
                    UiCommandKind::Release => {
                        self.handle_release_created(&mut state, &id, parent_id.as_deref(), &event)
                    }
                }
                self.live.set_section("events", Self::rebuild_lines(&state));
            }
            EventChange::Updated { id, event, .. } => {
                let mut state = self.state.lock().expect("command event state poisoned");
                let resource_key = match state.event_roles.get(&id).cloned() {
                    Some(EventRole::ResourceChild(resource_key)) => Some(resource_key),
                    _ => None,
                };
                if let Some(resource_key) = resource_key {
                    match event {
                        AlienEvent::CompilingCode { language, progress } => {
                            if let Some(entry) = state.resources.get_mut(&resource_key) {
                                entry.detail = progress
                                    .clone()
                                    .or_else(|| Some(format!("compiling {language}")));
                                entry.progress = None;
                            }
                        }
                        AlienEvent::PushingImage { progress, image } => {
                            if let Some(entry) = state.resources.get_mut(&resource_key) {
                                entry.detail = progress
                                    .as_ref()
                                    .map(|p| p.operation.clone())
                                    .or_else(|| Some(format!("pushing {image}")));
                                entry.progress = progress.clone();
                            }
                        }
                        _ => {}
                    }
                }
                self.live.set_section("events", Self::rebuild_lines(&state));
            }
            EventChange::StateChanged { id, new_state, .. } => {
                let mut state = self.state.lock().expect("command event state poisoned");
                match state.event_roles.get(&id).cloned() {
                    Some(EventRole::Status) => match new_state {
                        EventState::Success | EventState::Failed { .. } => {
                            self.clear_status(&mut state);
                        }
                        _ => {}
                    },
                    Some(EventRole::ResourceScope(resource_key)) => match new_state {
                        EventState::Success => {
                            // Remove completed resources so they don't linger
                            state.resources.shift_remove(&resource_key);
                        }
                        EventState::Failed { error } => {
                            if let Some(entry) = state.resources.get_mut(&resource_key) {
                                entry.state = RowState::Failed;
                                entry.detail = error.as_ref().map(|e| e.message.clone());
                                entry.progress = None;
                            }
                        }
                        _ => {}
                    },
                    _ => {}
                };
                self.live.set_section("events", Self::rebuild_lines(&state));
            }
        }

        Ok(())
    }
}

fn resource_key_from_parent_or_child(state: &CommandEventState, parent_id: &str) -> Option<String> {
    match state.event_roles.get(parent_id) {
        Some(EventRole::ResourceScope(resource_key))
        | Some(EventRole::ResourceChild(resource_key)) => Some(resource_key.clone()),
        _ => None,
    }
}

fn build_resource_noun(
    resource_name: &str,
    resource_type: &str,
    related_resources: &[String],
) -> String {
    if !related_resources.is_empty() && related_resources.len() > 1 {
        format!("{resource_type} {} (shared)", related_resources.join(", "))
    } else {
        format!("{resource_type} {resource_name}")
    }
}

// ---------------------------------------------------------------------------
// Card rendering (pure string formatting — unchanged)
// ---------------------------------------------------------------------------

pub fn render_deployment_cards(cards: &[DevDeploymentCard]) -> String {
    let mut sorted = cards.to_vec();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));

    let rendered: Vec<String> = sorted.iter().map(render_single_card).collect();
    // Leading blank line separates cards from the steps above
    format!("\n{}", rendered.join("\n"))
}

pub fn render_single_card(card: &DevDeploymentCard) -> String {
    let (status_icon, status_label): (&str, &str) = match card.status {
        DeploymentStatus::Running => ("●", "running"),
        DeploymentStatus::Pending => ("◐", "pending"),
        DeploymentStatus::InitialSetup => ("◐", "initial setup"),
        DeploymentStatus::Provisioning => ("◐", "provisioning"),
        DeploymentStatus::UpdatePending | DeploymentStatus::Updating => ("◐", "updating"),
        DeploymentStatus::DeletePending | DeploymentStatus::Deleting => ("◐", "stopping"),
        _ => ("✗", "failed"),
    };

    // Build plain-text content lines (no ANSI) to measure widths accurately
    let name_col_width = card
        .resources
        .iter()
        .map(|r| r.name.len())
        .max()
        .unwrap_or(0)
        .max(4);

    let mut content_lines: Vec<String> = Vec::new();
    for resource in &card.resources {
        let value = resource.url.as_deref().unwrap_or("running");
        content_lines.push(format!(
            "  {:<width$}  {}",
            resource.name,
            value,
            width = name_col_width
        ));
    }

    if let Some(error) = &card.error {
        if !card.resources.is_empty() {
            content_lines.push(String::new());
        }
        content_lines.push(format!("  Error: {error}"));
    }

    // Platform label for the header (e.g. "AWS", "Local")
    let platform_label = card.platform.as_ref().map(|p| match p {
        Platform::Aws => "AWS",
        Platform::Gcp => "GCP",
        Platform::Azure => "Azure",
        Platform::Kubernetes => "K8s",
        Platform::Local => "Local",
        Platform::Test => "Test",
    });

    // Calculate inner width from all elements (all plain text, no ANSI).
    // Use measure_text_width() for correct Unicode display width.
    // Add 2 columns of right padding so content doesn't touch the border.
    let platform_plain = platform_label
        .map(|p| format!(" · {p}"))
        .unwrap_or_default();
    let header_plain = format!(
        "─ {}{} ── {} {} ─",
        card.name, platform_plain, status_icon, status_label
    );
    let max_content_width = content_lines
        .iter()
        .map(|l| measure_text_width(l) + 2)
        .max()
        .unwrap_or(0);
    let inner_width = max_content_width
        .max(measure_text_width(&header_plain))
        .max(30);

    // Top border: ╭─ name · AWS ──────── ● running ─╮
    let status_styled = match card.status {
        DeploymentStatus::Running => style(format!("{status_icon} {status_label}"))
            .green()
            .to_string(),
        DeploymentStatus::Pending
        | DeploymentStatus::InitialSetup
        | DeploymentStatus::Provisioning => style(format!("{status_icon} {status_label}"))
            .cyan()
            .to_string(),
        DeploymentStatus::UpdatePending
        | DeploymentStatus::Updating
        | DeploymentStatus::DeletePending
        | DeploymentStatus::Deleting => style(format!("{status_icon} {status_label}"))
            .yellow()
            .to_string(),
        _ => style(format!("{status_icon} {status_label}"))
            .red()
            .to_string(),
    };

    let platform_styled = platform_label
        .map(|p| format!(" · {}", style(p).dim()))
        .unwrap_or_default();
    let name_section_plain = format!("─ {}{} ", card.name, platform_plain);
    let name_section_styled = format!("─ {}{} ", card.name, platform_styled);
    let status_section_plain = format!(" {} {} ─", status_icon, status_label);
    let fill_len = inner_width
        .saturating_sub(measure_text_width(&name_section_plain))
        .saturating_sub(measure_text_width(&status_section_plain));
    let fill = "─".repeat(fill_len);
    let top = format!("╭{name_section_styled}{fill} {status_styled} ─╮");

    // Content rows: │  resource  url                    │
    let body: Vec<String> = content_lines
        .iter()
        .map(|line| {
            let pad = inner_width.saturating_sub(measure_text_width(line));
            format!("│{line}{:pad$}│", "")
        })
        .collect();

    // Bottom border: ╰──────────────────────────────────╯
    let bottom = format!("╰{}╯", "─".repeat(inner_width));

    let mut parts = vec![top];
    parts.extend(body);
    parts.push(bottom);
    parts.join("\n")
}

fn render_dev_actions_footer() -> String {
    format!(
        "\n{}  {}  {}",
        format!(
            "{} {} {}",
            command("alien dev release"),
            dim_label("→"),
            dim_label("push changes")
        ),
        format!(
            "{} {} {}",
            command("alien dev deploy"),
            dim_label("→"),
            dim_label("new deployment")
        ),
        format!(
            "{} {} {}",
            dim_label("Ctrl+C"),
            dim_label("→"),
            dim_label("stop")
        ),
    )
}

pub fn render_serve_actions_footer() -> String {
    format!(
        "\n{}  {}  {}",
        format!(
            "{} {} {}",
            command("alien release"),
            dim_label("→"),
            dim_label("push changes")
        ),
        format!(
            "{} {} {}",
            command("alien onboard <name>"),
            dim_label("→"),
            dim_label("new customer")
        ),
        format!(
            "{} {} {}",
            dim_label("Ctrl+C"),
            dim_label("→"),
            dim_label("stop")
        ),
    )
}

// ---------------------------------------------------------------------------
// Standalone spinner (for init.rs and similar simple cases)
// ---------------------------------------------------------------------------

pub struct Spinner {
    live: Option<LiveRegion>,
}

impl Spinner {
    pub fn new(message: &str) -> Self {
        if !supports_ansi() {
            eprint!("{message}... ");
            return Self { live: None };
        }

        let live = LiveRegion::new();
        live.set_section("spinner", vec![format!("{SPINNER_PLACEHOLDER} {message}")]);
        Self { live: Some(live) }
    }

    pub fn finish_and_clear(self) {
        if let Some(live) = &self.live {
            live.clear();
        } else {
            eprintln!();
        }
        // Drop happens here — LiveRegion stop + join, nothing left on screen
    }
}
