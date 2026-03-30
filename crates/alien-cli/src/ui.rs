use std::collections::HashMap;
use std::io::{self, IsTerminal};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use alien_core::{
    AlienEvent, DeploymentStatus, EventBus, EventChange, EventHandler, EventState, PushProgress,
    ResourceStatus, StackResourceState,
};
use alien_error::{AlienError, AlienErrorData};
use async_trait::async_trait;
use comfy_table::{
    modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL_CONDENSED, Attribute, Cell, Color,
    ContentArrangement, Table,
};
use console::style;
use indexmap::IndexMap;
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};

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

#[derive(Debug, Clone)]
pub struct DevDeploymentResourceRow {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct DevDeploymentRow {
    pub name: String,
    pub deployment_id: String,
    pub status: DeploymentStatus,
    pub resources: Vec<DevDeploymentResourceRow>,
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

pub struct DevDeploymentsScreen {
    progress: Option<MultiProgress>,
    status_bar: Option<ProgressBar>,
    table_bar: Option<ProgressBar>,
    footer_bar: Option<ProgressBar>,
    focus_name: String,
}

impl DevDeploymentsScreen {
    pub fn new(focus_name: impl Into<String>) -> Self {
        let focus_name = focus_name.into();
        if !supports_ansi() {
            return Self {
                progress: None,
                status_bar: None,
                table_bar: None,
                footer_bar: None,
                focus_name,
            };
        }

        let progress = MultiProgress::with_draw_target(ProgressDrawTarget::stderr());
        let status_bar = progress.add(new_spinner_row());
        let table_bar = progress.add(new_block_row());
        let footer_bar = progress.add(new_block_row());

        footer_bar.set_message(render_dev_footer(&focus_name));

        Self {
            progress: Some(progress),
            status_bar: Some(status_bar),
            table_bar: Some(table_bar),
            footer_bar: Some(footer_bar),
            focus_name,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.progress.is_some()
    }

    pub fn update(&self, status_message: &str, deployments: &[DevDeploymentRow]) {
        let Some(status_bar) = &self.status_bar else {
            return;
        };
        let Some(table_bar) = &self.table_bar else {
            return;
        };
        let Some(footer_bar) = &self.footer_bar else {
            return;
        };

        status_bar.set_style(spinner_row_style());
        status_bar.set_message(status_message.to_string());
        table_bar.set_style(block_row_style());
        table_bar.set_message(render_dev_deployments_table(deployments));
        footer_bar.set_style(block_row_style());
        footer_bar.set_message(render_dev_footer(&self.focus_name));
    }

    pub fn println(&self, line: &str) {
        if let Some(progress) = &self.progress {
            let _ = progress.println(line);
        } else {
            println!("{line}");
        }
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
        format!("{} {}", style("OK").green().bold(), message)
    } else {
        format!("OK {message}")
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
    let headline_code = if supports_ansi() {
        style(format!("[{}]", report.code)).dim().to_string()
    } else {
        format!("[{}]", report.code)
    };

    let mut rendered = if supports_ansi() {
        format!(
            "{} {} {}",
            style("Error:").red().bold(),
            report.message,
            headline_code
        )
    } else {
        format!("Error: {} {}", report.message, headline_code)
    };

    if !report.causes.is_empty() {
        rendered.push('\n');
        rendered.push_str("Cause:");
        for cause in report.causes {
            rendered.push('\n');
            let code = if supports_ansi() {
                style(format!("[{}]", cause.code)).dim().to_string()
            } else {
                format!("[{}]", cause.code)
            };
            rendered.push_str(&format!("  - {} {}", cause.message, code));
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    Pending,
    Active,
    Complete,
    Failed,
    Skipped,
}

#[derive(Clone)]
pub struct FixedSteps {
    board: Option<Arc<ProgressBoard>>,
}

impl FixedSteps {
    pub fn new(step_labels: &[&str]) -> Self {
        if !supports_ansi() {
            return Self { board: None };
        }

        Self {
            board: Some(Arc::new(ProgressBoard::new(step_labels))),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.board.is_some()
    }

    pub fn activate(&self, index: usize, detail: Option<impl Into<String>>) {
        if let Some(board) = &self.board {
            board.set_step(index, RowState::Active, detail.map(Into::into));
        }
    }

    pub fn complete(&self, index: usize, detail: Option<impl Into<String>>) {
        if let Some(board) = &self.board {
            board.set_step(index, RowState::Complete, detail.map(Into::into));
        }
    }

    pub fn fail(&self, index: usize, detail: Option<impl Into<String>>) {
        if let Some(board) = &self.board {
            board.set_step(index, RowState::Failed, detail.map(Into::into));
        }
    }

    pub fn skip(&self, index: usize, detail: Option<impl Into<String>>) {
        if let Some(board) = &self.board {
            board.set_step(index, RowState::Skipped, detail.map(Into::into));
        }
    }

    pub fn resource_active(&self, key: &str, label: impl Into<String>, detail: Option<String>) {
        if let Some(board) = &self.board {
            board.set_resource_text(key, label.into(), RowState::Active, detail);
        }
    }

    pub fn resource_complete(&self, key: &str, detail: Option<String>) {
        if let Some(board) = &self.board {
            board.finish_resource(key, RowState::Complete, detail);
        }
    }

    pub fn resource_fail(&self, key: &str, detail: Option<String>) {
        if let Some(board) = &self.board {
            board.finish_resource(key, RowState::Failed, detail);
        }
    }

    pub fn resource_progress(
        &self,
        key: &str,
        label: impl Into<String>,
        detail: Option<String>,
        progress: &PushProgress,
    ) {
        if let Some(board) = &self.board {
            board.set_resource_progress(key, label.into(), detail, progress);
        }
    }

    pub fn println(&self, line: &str) {
        if let Some(board) = &self.board {
            board.println(line);
        } else {
            println!("{line}");
        }
    }

    pub fn sync_deployment_resources(
        &self,
        resources: &std::collections::HashMap<String, StackResourceState>,
    ) {
        if let Some(board) = &self.board {
            board.sync_deployment_resources(resources);
        }
    }
}

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

struct ProgressBoard {
    progress: MultiProgress,
    state: Mutex<ProgressBoardState>,
}

struct ProgressBoardState {
    step_labels: Vec<String>,
    step_states: Vec<RowState>,
    step_bars: Vec<Option<ProgressBar>>,
    resources: IndexMap<String, ResourceEntry>,
}

struct ResourceEntry {
    label: String,
    bar: ProgressBar,
    state: RowState,
}

impl ProgressBoard {
    fn new(step_labels: &[&str]) -> Self {
        let progress = MultiProgress::with_draw_target(ProgressDrawTarget::stderr());

        Self {
            progress,
            state: Mutex::new(ProgressBoardState {
                step_labels: step_labels
                    .iter()
                    .map(|label| (*label).to_string())
                    .collect(),
                step_states: vec![RowState::Pending; step_labels.len()],
                step_bars: vec![None; step_labels.len()],
                resources: IndexMap::new(),
            }),
        }
    }

    fn println(&self, line: &str) {
        let _ = self.progress.println(line);
    }

    fn set_step(&self, index: usize, state: RowState, detail: Option<String>) {
        let mut guard = self.state.lock().expect("progress board lock poisoned");
        if let Some(label) = guard.step_labels.get(index).cloned() {
            guard.step_states[index] = state;
            if matches!(state, RowState::Pending) && guard.step_bars[index].is_none() {
                return;
            }

            let bar = self.ensure_step_bar(&mut guard, index);
            apply_step_row(&bar, state, &label, detail.as_deref());
        }
    }

    fn ensure_step_bar(&self, state: &mut ProgressBoardState, index: usize) -> ProgressBar {
        if let Some(bar) = &state.step_bars[index] {
            return bar.clone();
        }

        let bar = self.progress.add(new_text_row());
        state.step_bars[index] = Some(bar.clone());
        bar
    }

    fn set_resource_text(&self, key: &str, label: String, state: RowState, detail: Option<String>) {
        let mut guard = self.state.lock().expect("progress board lock poisoned");
        let entry = guard
            .resources
            .entry(key.to_string())
            .or_insert_with(|| ResourceEntry {
                label: label.clone(),
                bar: self.progress.add(new_text_row()),
                state: RowState::Pending,
            });
        entry.label = label;
        entry.state = state;
        apply_text_row(&entry.bar, state, &entry.label, detail.as_deref());
    }

    fn set_resource_progress(
        &self,
        key: &str,
        label: String,
        detail: Option<String>,
        progress: &PushProgress,
    ) {
        let mut guard = self.state.lock().expect("progress board lock poisoned");
        let entry = guard
            .resources
            .entry(key.to_string())
            .or_insert_with(|| ResourceEntry {
                label: label.clone(),
                bar: self.progress.add(new_text_row()),
                state: RowState::Pending,
            });
        entry.label = label;
        entry.state = RowState::Active;

        if progress.total_bytes > 0 {
            entry.bar.set_style(bytes_progress_style());
            entry.bar.set_length(progress.total_bytes);
            entry
                .bar
                .set_position(progress.bytes_uploaded.min(progress.total_bytes));
        } else if progress.total_layers > 0 {
            entry.bar.set_style(layers_progress_style());
            entry.bar.set_length(progress.total_layers as u64);
            entry
                .bar
                .set_position(progress.layers_uploaded.min(progress.total_layers) as u64);
        } else {
            entry.bar.set_style(text_row_style());
        }

        let message = format_message(&entry.label, detail.as_deref());
        entry.bar.set_prefix(active_prefix());
        entry.bar.set_message(message);
    }

    fn finish_resource(&self, key: &str, state: RowState, detail: Option<String>) {
        let mut guard = self.state.lock().expect("progress board lock poisoned");
        if let Some(entry) = guard.resources.get_mut(key) {
            entry.state = state;
            entry.bar.set_style(text_row_style());
            apply_text_row(&entry.bar, state, &entry.label, detail.as_deref());
        }
    }

    fn sync_deployment_resources(
        &self,
        resources: &std::collections::HashMap<String, StackResourceState>,
    ) {
        let mut entries: Vec<_> = resources.iter().collect();
        entries.sort_by(|(left_name, _), (right_name, _)| left_name.cmp(right_name));

        for (resource_name, resource) in entries {
            let key = format!("deployment:{resource_name}");
            let label = deployment_resource_label(resource_name, resource);
            let detail = deployment_resource_detail(resource);
            let state = match resource.status {
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

            self.set_resource_text(
                &key,
                label,
                state,
                Some(detail.unwrap_or_else(|| format_resource_status(resource.status).to_string())),
            );
        }
    }
}

fn new_text_row() -> ProgressBar {
    let bar = ProgressBar::new(1);
    bar.set_style(text_row_style());
    bar
}

fn new_block_row() -> ProgressBar {
    let bar = ProgressBar::new(1);
    bar.set_style(block_row_style());
    bar
}

fn text_row_style() -> ProgressStyle {
    ProgressStyle::with_template("{prefix} {msg}").expect("text row template should be valid")
}

fn block_row_style() -> ProgressStyle {
    ProgressStyle::with_template("{msg}").expect("block row template should be valid")
}

fn bytes_progress_style() -> ProgressStyle {
    ProgressStyle::with_template("{prefix:>3} {msg} [{bar:24.cyan/blue}] {percent:>3}%")
        .expect("bytes progress template should be valid")
}

fn layers_progress_style() -> ProgressStyle {
    ProgressStyle::with_template("{prefix:>3} {msg} [{bar:24.cyan/blue}] {pos}/{len}")
        .expect("layers progress template should be valid")
}

fn apply_text_row(bar: &ProgressBar, state: RowState, label: &str, detail: Option<&str>) {
    bar.disable_steady_tick();
    bar.set_style(text_row_style());
    bar.set_prefix(prefix_for_state(state));
    bar.set_message(format_message(label, detail));
}

fn apply_step_row(bar: &ProgressBar, state: RowState, label: &str, detail: Option<&str>) {
    match state {
        RowState::Active => {
            bar.set_style(spinner_row_style());
            bar.set_message(format_message(label, detail));
            bar.enable_steady_tick(Duration::from_millis(120));
        }
        _ => apply_text_row(bar, state, label, detail),
    }
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
        style("OK").green().bold().to_string()
    } else {
        "OK".to_string()
    }
}

fn error_prefix() -> String {
    if supports_ansi() {
        style("ERR").red().bold().to_string()
    } else {
        "ERR".to_string()
    }
}

fn format_message(label: &str, detail: Option<&str>) -> String {
    match detail {
        Some(detail) if !detail.is_empty() => format!("{label} {detail}"),
        _ => label.to_string(),
    }
}

struct CommandEventHandler {
    kind: UiCommandKind,
    progress: MultiProgress,
    state: Mutex<CommandEventState>,
}

struct CommandEventState {
    status_bar: Option<ProgressBar>,
    event_roles: HashMap<String, EventRole>,
    resources: IndexMap<String, LiveResourceEntry>,
}

#[derive(Clone)]
enum EventRole {
    Status,
    ResourceScope(String),
    ResourceChild(String),
}

struct LiveResourceEntry {
    label: String,
    bar: ProgressBar,
}

impl CommandEventHandler {
    fn new(kind: UiCommandKind) -> Self {
        Self {
            kind,
            progress: MultiProgress::with_draw_target(ProgressDrawTarget::stderr()),
            state: Mutex::new(CommandEventState {
                status_bar: None,
                event_roles: HashMap::new(),
                resources: IndexMap::new(),
            }),
        }
    }

    fn ensure_status_bar(&self, state: &mut CommandEventState) -> ProgressBar {
        if let Some(bar) = &state.status_bar {
            return bar.clone();
        }

        let bar = self.progress.add(new_spinner_row());
        state.status_bar = Some(bar.clone());
        bar
    }

    fn set_status(&self, state: &mut CommandEventState, message: impl Into<String>) {
        let bar = self.ensure_status_bar(state);
        bar.set_style(spinner_row_style());
        bar.set_message(message.into());
    }

    fn clear_status(&self, state: &mut CommandEventState) {
        if let Some(bar) = state.status_bar.take() {
            bar.finish_and_clear();
        }
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
                let entry =
                    state
                        .resources
                        .entry(key.clone())
                        .or_insert_with(|| LiveResourceEntry {
                            label: label.clone(),
                            bar: self.progress.add(new_spinner_row()),
                        });
                entry.label = label.clone();
                entry.bar.set_style(spinner_row_style());
                entry
                    .bar
                    .set_message(active_resource_message(&key, &label, None));
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
                        let detail = progress
                            .clone()
                            .or_else(|| Some(format!("compiling {language}")));
                        entry.bar.set_message(active_resource_message(
                            &resource_key,
                            &entry.label,
                            detail.as_deref(),
                        ));
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
                let entry =
                    state
                        .resources
                        .entry(key.clone())
                        .or_insert_with(|| LiveResourceEntry {
                            label: label.clone(),
                            bar: self.progress.add(new_spinner_row()),
                        });
                entry.label = label.clone();
                entry.bar.set_style(spinner_row_style());
                entry
                    .bar
                    .set_message(active_resource_message(&key, &label, None));
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
                        let detail = progress
                            .clone()
                            .or_else(|| Some(format!("compiling {language}")));
                        entry.bar.set_message(active_resource_message(
                            &resource_key,
                            &entry.label,
                            detail.as_deref(),
                        ));
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
                let entry =
                    state
                        .resources
                        .entry(key.clone())
                        .or_insert_with(|| LiveResourceEntry {
                            label: label.clone(),
                            bar: self.progress.add(new_spinner_row()),
                        });
                entry.label = label.clone();
                entry.bar.set_style(spinner_row_style());
                entry
                    .bar
                    .set_message(active_resource_message(&key, &label, Some("preparing")));
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
                        let detail = progress
                            .as_ref()
                            .map(|progress| progress.operation.clone())
                            .or_else(|| Some(format!("pushing {image}")));
                        if let Some(progress) = progress {
                            apply_push_progress(
                                &entry.bar,
                                &resource_key,
                                &entry.label,
                                detail.as_deref(),
                                progress,
                            );
                        } else {
                            entry.bar.set_style(spinner_row_style());
                            entry.bar.set_message(active_resource_message(
                                &resource_key,
                                &entry.label,
                                detail.as_deref(),
                            ));
                        }
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
                                let detail = progress
                                    .clone()
                                    .or_else(|| Some(format!("compiling {language}")));
                                entry.bar.set_style(spinner_row_style());
                                entry.bar.set_message(active_resource_message(
                                    &resource_key,
                                    &entry.label,
                                    detail.as_deref(),
                                ));
                            }
                        }
                        AlienEvent::PushingImage { progress, image } => {
                            if let Some(entry) = state.resources.get_mut(&resource_key) {
                                let detail = progress
                                    .as_ref()
                                    .map(|progress| progress.operation.clone())
                                    .or_else(|| Some(format!("pushing {image}")));
                                if let Some(progress) = progress {
                                    apply_push_progress(
                                        &entry.bar,
                                        &resource_key,
                                        &entry.label,
                                        detail.as_deref(),
                                        &progress,
                                    );
                                } else {
                                    entry.bar.set_style(spinner_row_style());
                                    entry.bar.set_message(active_resource_message(
                                        &resource_key,
                                        &entry.label,
                                        detail.as_deref(),
                                    ));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            EventChange::StateChanged { id, new_state, .. } => {
                let mut state = self.state.lock().expect("command event state poisoned");
                match state.event_roles.get(&id).cloned() {
                    Some(EventRole::Status) => match new_state {
                        EventState::Success => {}
                        EventState::Failed { .. } => self.clear_status(&mut state),
                        _ => {}
                    },
                    Some(EventRole::ResourceScope(resource_key)) => match new_state {
                        EventState::Success => {
                            if let Some(entry) = state.resources.get_mut(&resource_key) {
                                entry.bar.finish_and_clear();
                            }
                        }
                        EventState::Failed { error } => {
                            if let Some(entry) = state.resources.get_mut(&resource_key) {
                                entry.bar.set_style(text_row_style());
                                entry.bar.set_prefix(error_prefix());
                                entry.bar.set_message(failed_resource_message(
                                    &resource_key,
                                    &entry.label,
                                    error.as_ref().map(|error| error.message.as_str()),
                                ));
                                entry.bar.abandon();
                            }
                        }
                        _ => {}
                    },
                    _ => {}
                };
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

fn action_for_resource(resource_key: &str) -> (&'static str, &'static str) {
    if resource_key.starts_with("push:") {
        ("Pushing", "Pushed")
    } else {
        ("Building", "Built")
    }
}

fn active_resource_message(resource_key: &str, label: &str, detail: Option<&str>) -> String {
    let (active, _) = action_for_resource(resource_key);
    format_message(&format!("{active} {label}"), detail)
}

fn failed_resource_message(resource_key: &str, label: &str, detail: Option<&str>) -> String {
    let (active, _) = action_for_resource(resource_key);
    if let Some(detail) = detail {
        format!("{} {}", format!("{active} {label}"), detail)
    } else {
        format!("{active} {label}")
    }
}

fn spinner_row_style() -> ProgressStyle {
    ProgressStyle::with_template("{spinner:.cyan} {msg}")
        .expect("spinner row template should be valid")
        .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
}

fn new_spinner_row() -> ProgressBar {
    let bar = ProgressBar::new_spinner();
    bar.set_style(spinner_row_style());
    bar.enable_steady_tick(Duration::from_millis(120));
    bar
}

fn apply_push_progress(
    bar: &ProgressBar,
    resource_key: &str,
    label: &str,
    detail: Option<&str>,
    progress: &PushProgress,
) {
    if progress.total_bytes > 0 {
        bar.set_style(
            ProgressStyle::with_template(
                "{spinner:.cyan} {msg} [{wide_bar:.cyan/blue}] {bytes}/{total_bytes}",
            )
            .expect("bytes progress template should be valid")
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
        );
        bar.set_length(progress.total_bytes);
        bar.set_position(progress.bytes_uploaded.min(progress.total_bytes));
    } else if progress.total_layers > 0 {
        bar.set_style(
            ProgressStyle::with_template(
                "{spinner:.cyan} {msg} [{wide_bar:.cyan/blue}] {pos}/{len}",
            )
            .expect("layers progress template should be valid")
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
        );
        bar.set_length(progress.total_layers as u64);
        bar.set_position(progress.layers_uploaded.min(progress.total_layers) as u64);
    } else {
        bar.set_style(spinner_row_style());
    }

    bar.set_message(active_resource_message(resource_key, label, detail));
}

fn deployment_resource_label(resource_name: &str, resource: &StackResourceState) -> String {
    format!("{resource_name} ({})", resource.resource_type)
}

fn render_dev_deployments_table(deployments: &[DevDeploymentRow]) -> String {
    let mut table = make_table(&["Name", "Deployment ID", "Status", "Resources"]);

    let mut sorted = deployments.to_vec();
    sorted.sort_by(|left, right| left.name.cmp(&right.name));

    for deployment in sorted {
        let mut resources = deployment.resources;
        resources.sort_by(|left, right| left.name.cmp(&right.name));

        let resources_cell = if resources.is_empty() {
            "no public URL yet".to_string()
        } else {
            resources
                .into_iter()
                .map(|resource| format!("{}: {}", resource.name, resource.url))
                .collect::<Vec<_>>()
                .join("\n")
        };

        table.add_row(vec![
            deployment.name.into(),
            deployment.deployment_id.into(),
            status_cell(format_deployment_status(deployment.status)),
            Cell::new(resources_cell),
        ]);
    }

    table.to_string()
}

fn render_dev_footer(focus_name: &str) -> String {
    format!(
        "{}\n  Ship a change with {}\n  Create another deployment with {}\n  Inspect {} with {}\n{}",
        dim_label("Try next"),
        command("alien dev release"),
        command("alien dev deploy --name preview --platform local"),
        focus_name,
        command(&format!("alien dev deployments get {focus_name}")),
        dim_label("Watching local deployments for live updates. Press Ctrl+C to stop this session.")
    )
}
