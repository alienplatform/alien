//! Self-contained build UI component
//!
//! Displays build progress for functions, containers, and other resource types.
//! Handles event-driven state updates, spinner animations, and dynamic viewport sizing.

use crate::error::{ErrorData, Result};
use crate::tui::common::{
    widgets, StepState, StepStatus, MAX_VIEWPORT_HEIGHT, MIN_VIEWPORT_HEIGHT, SPINNER_FRAMES,
};
use crate::tui::ErrorPrinter;
use alien_core::{AlienEvent, EventChange, EventState};
use alien_error::{AlienError, GenericError};
use ratatui::{
    crossterm::event::{self, KeyModifiers},
    prelude::*,
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Terminal, TerminalOptions, Viewport,
};
use std::{
    collections::HashMap,
    io::Write,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

/// Ordered list of build step keys
const STEP_ORDER: &[&str] = &["configuration", "preflights", "build", "template"];

/// Type of compute resource being built
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ResourceType {
    Function,
    Container,
    Worker,
}

impl ResourceType {
    fn from_str(s: &str) -> Self {
        match s {
            "container" => Self::Container,
            "worker" => Self::Worker,
            _ => Self::Function,
        }
    }

    fn plural(&self) -> &'static str {
        match self {
            Self::Function => "Functions",
            Self::Container => "Containers",
            Self::Worker => "Workers",
        }
    }
}

/// State of a resource being built
#[derive(Debug, Clone)]
pub struct ResourceBuildState {
    /// Resource name
    pub name: String,
    /// Resource type
    pub resource_type: ResourceType,
    /// Current build phase
    pub phase: BuildPhase,
    /// Start time
    pub start_time: Option<Instant>,
    /// End time
    pub end_time: Option<Instant>,
}

impl ResourceBuildState {
    /// Calculate duration if both start and end times are available
    pub fn duration(&self) -> Option<f64> {
        match (self.start_time, self.end_time) {
            (Some(start), Some(end)) => Some(end.duration_since(start).as_secs_f64()),
            _ => None,
        }
    }
}

/// Current phase of resource building
#[derive(Debug, Clone)]
pub enum BuildPhase {
    Queued,
    Compiling {
        /// Current compilation output line
        progress: Option<String>,
    },
    Completed {
        /// Total duration in seconds
        duration: f64,
    },
    Failed {
        /// Error message
        error: String,
    },
    Canceled,
}

/// Final build result
#[derive(Debug, Clone)]
pub enum BuildResult {
    Success,
    Failed(AlienError<GenericError>),
}

/// Represents the overall state of the build process
#[derive(Debug, Clone)]
pub struct BuildState {
    /// Main build steps and their current status
    pub steps: HashMap<String, StepState>,
    /// Resources being built and their progress (keyed by resource name)
    pub resources: HashMap<String, ResourceBuildState>,
    /// Track parent-child relationships for event hierarchy
    pub event_parents: HashMap<String, String>,
    /// Map BuildingResource event IDs to all resource names they cover
    pub resource_event_ids: HashMap<String, Vec<String>>,
    /// Map main step event IDs to step keys (e.g., "config-1" -> "configuration")
    pub step_event_ids: HashMap<String, String>,
    /// Current spinner frame for animations
    pub spinner_frame: usize,
    /// Build start time
    pub start_time: Instant,
    /// Build end time (if completed)
    pub end_time: Option<Instant>,
    /// Target platform name
    pub platform: String,
    /// Output directory
    pub output_dir: String,
    /// Final result
    pub result: Option<BuildResult>,
    /// Previous calculated height for viewport resizing
    pub previous_height: u16,
}

impl BuildState {
    /// Create a new build state
    pub fn new(platform: String, output_dir: String) -> Self {
        let mut steps = HashMap::new();

        steps.insert(
            "configuration".to_string(),
            StepState {
                name: "Load configuration".to_string(),
                status: StepStatus::Pending,
                warning_count: None,
            },
        );

        steps.insert(
            "preflights".to_string(),
            StepState {
                name: "Run preflights".to_string(),
                status: StepStatus::Pending,
                warning_count: None,
            },
        );

        steps.insert(
            "build".to_string(),
            StepState {
                name: "Build".to_string(),
                status: StepStatus::Pending,
                warning_count: None,
            },
        );

        steps.insert(
            "template".to_string(),
            StepState {
                name: "Generate template".to_string(),
                status: StepStatus::Pending,
                warning_count: None,
            },
        );

        let initial_height = Self::calculate_height_for_empty_state();

        Self {
            steps,
            resources: HashMap::new(),
            event_parents: HashMap::new(),
            resource_event_ids: HashMap::new(),
            step_event_ids: HashMap::new(),
            spinner_frame: 0,
            start_time: Instant::now(),
            end_time: None,
            platform,
            output_dir,
            result: None,
            previous_height: initial_height,
        }
    }

    /// Calculate initial height for an empty build state
    fn calculate_height_for_empty_state() -> u16 {
        // Header + empty line + 4 steps + trailing line
        let height: u16 = 2 + 4 + 1;
        height.max(MIN_VIEWPORT_HEIGHT).min(MAX_VIEWPORT_HEIGHT)
    }

    /// Update spinner animation
    pub fn tick(&mut self) {
        self.spinner_frame = (self.spinner_frame + 1) % SPINNER_FRAMES.len();
    }

    /// Check if the UI height has changed and update internal tracking.
    /// Returns the new height if it changed, None if unchanged.
    pub fn check_and_update_height(&mut self) -> Option<u16> {
        let new_height = BuildUi::calculate_height(self);
        let constrained = new_height.max(MIN_VIEWPORT_HEIGHT).min(MAX_VIEWPORT_HEIGHT);

        if constrained != self.previous_height {
            self.previous_height = constrained;
            Some(constrained)
        } else {
            None
        }
    }

    // ── Event handling ──────────────────────────────────────────────────

    /// Update state based on an alien event change
    pub fn handle_event_change(&mut self, event_change: &EventChange) {
        match event_change {
            EventChange::Created {
                id,
                parent_id,
                event,
                state,
                ..
            } => {
                if let Some(parent) = parent_id {
                    self.event_parents.insert(id.clone(), parent.clone());
                }

                // Track BuildingResource events: map event ID to all resource names
                if let AlienEvent::BuildingResource {
                    resource_name,
                    related_resources,
                    ..
                } = event
                {
                    let mut names = vec![resource_name.clone()];
                    // Add related resources that aren't the primary (dedup expansion)
                    for r in related_resources {
                        if r != resource_name && !names.contains(r) {
                            names.push(r.clone());
                        }
                    }
                    self.resource_event_ids.insert(id.clone(), names);
                }

                // Track main step event IDs for later StateChanged lookup
                let step_key = match event {
                    AlienEvent::LoadingConfiguration => Some("configuration"),
                    AlienEvent::RunningPreflights { .. } => Some("preflights"),
                    AlienEvent::GeneratingTemplate { .. } => Some("template"),
                    _ => None,
                };
                if let Some(key) = step_key {
                    self.step_event_ids.insert(id.clone(), key.to_string());
                }

                self.handle_event_created(id, event, state);
            }
            EventChange::Updated { id, event, .. } => {
                self.handle_event_created(id, event, &EventState::None);
            }
            EventChange::StateChanged { id, new_state, .. } => {
                self.handle_state_change(id, new_state);
            }
        }
    }

    /// Handle a newly created or updated event
    fn handle_event_created(&mut self, event_id: &str, event: &AlienEvent, state: &EventState) {
        match event {
            AlienEvent::LoadingConfiguration => {
                self.update_step("configuration", state);
            }
            AlienEvent::RunningPreflights { .. } => {
                self.update_step("preflights", state);
            }
            AlienEvent::BuildingResource {
                resource_name,
                resource_type,
                related_resources,
            } => {
                if matches!(state, EventState::Started) {
                    // Mark build step as in progress
                    if let Some(step) = self.steps.get_mut("build") {
                        if matches!(step.status, StepStatus::Pending) {
                            step.status = StepStatus::InProgress;
                        }
                    }

                    let rt = ResourceType::from_str(resource_type);
                    let now = Some(Instant::now());

                    // Insert primary resource
                    self.resources.insert(
                        resource_name.clone(),
                        ResourceBuildState {
                            name: resource_name.clone(),
                            resource_type: rt.clone(),
                            phase: BuildPhase::Compiling { progress: None },
                            start_time: now,
                            end_time: None,
                        },
                    );

                    // Insert related resources (dedup expansion) with same state
                    for related in related_resources {
                        if related != resource_name {
                            self.resources.insert(
                                related.clone(),
                                ResourceBuildState {
                                    name: related.clone(),
                                    resource_type: rt.clone(),
                                    phase: BuildPhase::Compiling { progress: None },
                                    start_time: now,
                                    end_time: None,
                                },
                            );
                        }
                    }

                    // Update the build step name based on resource types present
                    self.update_build_step_name();
                }
            }
            AlienEvent::CompilingCode { progress, .. } => {
                // Find the resource associated with this event via parent hierarchy
                if let Some(resource_name) = self.get_resource_for_event(event_id) {
                    // Get all resources sharing this build event
                    let names = self.get_all_resources_for_primary(&resource_name);
                    for name in names {
                        if let Some(res) = self.resources.get_mut(&name) {
                            if matches!(res.phase, BuildPhase::Compiling { .. }) {
                                res.phase = BuildPhase::Compiling {
                                    progress: progress.clone(),
                                };
                            }
                        }
                    }
                }
            }
            AlienEvent::BuildingImage { .. } => {
                // Keep compiling status; completion handled by BuildingResource state change
            }
            AlienEvent::GeneratingTemplate { .. } => {
                self.update_step("template", state);
            }
            _ => {}
        }
    }

    /// Handle state transitions for existing events
    fn handle_state_change(&mut self, event_id: &str, new_state: &EventState) {
        // Check if this is a main step event
        if let Some(step_key) = self.step_event_ids.get(event_id).cloned() {
            match new_state {
                EventState::Success => {
                    if let Some(step) = self.steps.get_mut(&step_key) {
                        if matches!(step.status, StepStatus::InProgress) {
                            step.status = StepStatus::Completed;
                        }
                    }
                }
                EventState::Failed { error } => {
                    let msg = error
                        .as_ref()
                        .map(|e| e.message.clone())
                        .unwrap_or_else(|| "Unknown error".to_string());
                    if let Some(step) = self.steps.get_mut(&step_key) {
                        if matches!(step.status, StepStatus::InProgress) {
                            step.status = StepStatus::Failed(msg);
                        }
                    }
                }
                _ => {}
            }
        }

        // Check if this is a BuildingResource event
        if let Some(resource_names) = self.resource_event_ids.get(event_id).cloned() {
            match new_state {
                EventState::Success => {
                    let now = Instant::now();
                    for name in &resource_names {
                        if let Some(res) = self.resources.get_mut(name) {
                            res.end_time = Some(now);
                            if let Some(duration) = res.duration() {
                                res.phase = BuildPhase::Completed { duration };
                            }
                        }
                    }

                    if self.all_resources_complete() {
                        if let Some(step) = self.steps.get_mut("build") {
                            step.status = StepStatus::Completed;
                        }
                    }
                }
                EventState::Failed { error } => {
                    let msg = error
                        .as_ref()
                        .map(|e| e.message.clone())
                        .unwrap_or_else(|| "Unknown error".to_string());
                    let now = Instant::now();

                    for name in &resource_names {
                        if let Some(res) = self.resources.get_mut(name) {
                            res.end_time = Some(now);
                            res.phase = BuildPhase::Failed { error: msg.clone() };
                        }
                    }

                    self.cancel_remaining_resources();

                    if let Some(step) = self.steps.get_mut("build") {
                        step.status = StepStatus::Failed("Build failed".to_string());
                    }
                }
                _ => {}
            }
        }
    }

    /// Update a main step's status from an event state
    fn update_step(&mut self, step_key: &str, state: &EventState) {
        if let Some(step) = self.steps.get_mut(step_key) {
            step.status = match state {
                EventState::Started => StepStatus::InProgress,
                EventState::Success => StepStatus::Completed,
                EventState::Failed { error } => StepStatus::Failed(
                    error
                        .as_ref()
                        .map(|e| e.message.clone())
                        .unwrap_or_else(|| "Unknown error".to_string()),
                ),
                _ => return,
            };
        }
    }

    /// Get the primary resource name associated with an event ID by traversing hierarchy
    fn get_resource_for_event(&self, event_id: &str) -> Option<String> {
        // Check if this event directly maps to resources
        if let Some(names) = self.resource_event_ids.get(event_id) {
            return names.first().cloned();
        }

        // Traverse up parent chain (max 10 levels to avoid loops)
        let mut current = event_id;
        for _ in 0..10 {
            if let Some(names) = self.resource_event_ids.get(current) {
                return names.first().cloned();
            }
            match self.event_parents.get(current) {
                Some(parent) => current = parent,
                None => break,
            }
        }

        None
    }

    /// Get all resource names that share a build with the given primary resource.
    /// Returns at least the primary itself.
    fn get_all_resources_for_primary(&self, primary: &str) -> Vec<String> {
        for names in self.resource_event_ids.values() {
            if names.contains(&primary.to_string()) {
                return names.clone();
            }
        }
        vec![primary.to_string()]
    }

    /// Update the "Build" step name based on what resource types are present
    fn update_build_step_name(&mut self) {
        let name = self.build_section_label();
        if let Some(step) = self.steps.get_mut("build") {
            step.name = name;
        }
    }

    /// Determine the section label based on resource types present
    fn build_section_label(&self) -> String {
        let types: Vec<&ResourceType> = self
            .resources
            .values()
            .map(|r| &r.resource_type)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        if types.len() == 1 {
            format!("Build {}", types[0].plural().to_lowercase())
        } else {
            "Build".to_string()
        }
    }

    /// Determine the section header for the resources list
    fn resources_section_header(&self) -> String {
        let types: Vec<&ResourceType> = self
            .resources
            .values()
            .map(|r| &r.resource_type)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        let count = self.resources.len();
        let label = if types.len() == 1 {
            types[0].plural().to_string()
        } else {
            "Build targets".to_string()
        };

        format!("{} ({} total):", label, count)
    }

    /// Set the final build result
    pub fn set_result(&mut self, result: BuildResult) {
        self.result = Some(result);
        self.end_time = Some(Instant::now());
    }

    /// Set an error result from any AlienError type
    pub fn set_error<T>(&mut self, error: AlienError<T>)
    where
        T: alien_error::AlienErrorData + Clone + std::fmt::Debug + serde::Serialize,
    {
        self.end_time = Some(Instant::now());
        self.set_result(BuildResult::Failed(error.into_generic()));
    }

    /// Cancel all non-completed resource builds
    pub fn cancel_remaining_resources(&mut self) {
        for res in self.resources.values_mut() {
            if matches!(res.phase, BuildPhase::Queued | BuildPhase::Compiling { .. }) {
                res.phase = BuildPhase::Canceled;
                res.end_time = Some(Instant::now());
            }
        }
    }

    /// Check if all resources are complete
    fn all_resources_complete(&self) -> bool {
        !self.resources.is_empty()
            && self.resources.values().all(|r| {
                matches!(
                    r.phase,
                    BuildPhase::Completed { .. } | BuildPhase::Failed { .. } | BuildPhase::Canceled
                )
            })
    }

    /// Get elapsed time
    pub fn elapsed(&self) -> Duration {
        self.end_time.unwrap_or_else(Instant::now) - self.start_time
    }

    /// Index of the latest active/completed step in STEP_ORDER
    fn current_step_index(&self) -> usize {
        let mut last = 0;
        for (i, key) in STEP_ORDER.iter().enumerate() {
            if let Some(step) = self.steps.get(*key) {
                if !matches!(step.status, StepStatus::Pending) {
                    last = i;
                }
            }
        }
        last
    }
}

// ── Props, events, component ────────────────────────────────────────────

/// Props interface for the BuildUiComponent
#[derive(Debug, Clone)]
pub struct BuildUiProps {
    pub platform: String,
    pub output_dir: String,
    pub on_result: Option<fn(BuildResult)>,
    pub on_cancel: Option<fn()>,
}

/// Events that can be sent to the BuildUiComponent
#[derive(Debug)]
pub enum BuildUiEvent {
    AlienEventChange(EventChange),
    BuildFinished(std::result::Result<BuildResult, AlienError<crate::error::ErrorData>>),
    Cancel,
}

/// Internal TUI events
#[derive(Debug)]
enum InternalTuiEvent {
    Input(event::KeyEvent),
    Tick,
    Resize,
    BuildUiEvent(BuildUiEvent),
}

/// Self-contained build UI component
pub struct BuildUiComponent {
    state: BuildState,
    terminal: Option<Terminal<CrosstermBackend<std::io::Stdout>>>,
    event_tx: Option<mpsc::Sender<InternalTuiEvent>>,
    event_rx: Option<mpsc::Receiver<InternalTuiEvent>>,
    running: bool,
}

impl BuildUiComponent {
    pub fn new(props: BuildUiProps) -> Self {
        Self {
            state: BuildState::new(props.platform, props.output_dir),
            terminal: None,
            event_tx: None,
            event_rx: None,
            running: false,
        }
    }

    pub fn start(&mut self) -> Result<mpsc::Sender<BuildUiEvent>> {
        if self.running {
            return Err(AlienError::new(ErrorData::TuiOperationFailed {
                message: "BuildUiComponent is already running".to_string(),
            }));
        }

        let terminal = ratatui::init_with_options(TerminalOptions {
            viewport: Viewport::Inline(MIN_VIEWPORT_HEIGHT),
        });
        self.terminal = Some(terminal);

        let (internal_tx, internal_rx) = mpsc::channel();
        let (external_tx, external_rx) = mpsc::channel();

        self.event_rx = Some(internal_rx);
        self.running = true;

        // Input handler thread
        let input_tx = internal_tx.clone();
        thread::spawn(move || Self::input_handler_thread(input_tx));

        // External event forwarder
        let forward_tx = internal_tx.clone();
        thread::spawn(move || {
            while let Ok(evt) = external_rx.recv() {
                if forward_tx
                    .send(InternalTuiEvent::BuildUiEvent(evt))
                    .is_err()
                {
                    break;
                }
            }
        });

        self.event_tx = Some(internal_tx);
        Ok(external_tx)
    }

    pub fn stop(&mut self) {
        self.running = false;
        if self.terminal.is_some() {
            ratatui::restore();
            self.terminal = None;
        }
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
    }

    pub fn run_event_loop(&mut self) -> Result<()> {
        if !self.running || self.terminal.is_none() || self.event_rx.is_none() {
            return Err(AlienError::new(ErrorData::TuiOperationFailed {
                message: "BuildUiComponent not properly initialized".to_string(),
            }));
        }

        let mut redraw = true;
        let mut build_result: Option<
            std::result::Result<BuildResult, AlienError<crate::error::ErrorData>>,
        > = None;
        let mut show_result_until: Option<Instant> = None;
        let mut stop_animation = false;

        while self.running {
            if redraw {
                if let Some(ref mut terminal) = self.terminal {
                    let _ = terminal.draw(|frame| BuildUi::draw(frame, &self.state));
                }
                redraw = false;
            }

            if let Some(until) = show_result_until {
                if Instant::now() >= until {
                    break;
                }
            }

            if let Some(ref rx) = self.event_rx {
                match rx.recv_timeout(Duration::from_millis(80)) {
                    Ok(InternalTuiEvent::Input(key)) => {
                        if key.code == event::KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL)
                        {
                            break;
                        }
                    }
                    Ok(InternalTuiEvent::Tick) => {
                        if !stop_animation {
                            self.state.tick();
                            redraw = true;
                        }
                    }
                    Ok(InternalTuiEvent::Resize) => {
                        redraw = true;
                    }
                    Ok(InternalTuiEvent::BuildUiEvent(BuildUiEvent::AlienEventChange(change))) => {
                        self.state.handle_event_change(&change);
                        if let Some(new_height) = self.state.check_and_update_height() {
                            if let Some(ref mut terminal) = self.terminal {
                                let _ = terminal.set_viewport_height(new_height);
                            }
                        }
                        redraw = true;
                    }
                    Ok(InternalTuiEvent::BuildUiEvent(BuildUiEvent::BuildFinished(result))) => {
                        build_result = Some(result.clone());
                        match &result {
                            Ok(res) => {
                                self.state.set_result(res.clone());
                                if let Some(new_height) = self.state.check_and_update_height() {
                                    if let Some(ref mut terminal) = self.terminal {
                                        let _ = terminal.set_viewport_height(new_height);
                                    }
                                }
                                stop_animation = true;
                                show_result_until = Some(Instant::now());
                                redraw = true;
                            }
                            Err(error) => {
                                self.state.set_error(error.clone());
                                if let Some(new_height) = self.state.check_and_update_height() {
                                    if let Some(ref mut terminal) = self.terminal {
                                        let _ = terminal.set_viewport_height(new_height);
                                    }
                                }
                                stop_animation = true;
                                show_result_until =
                                    Some(Instant::now() + Duration::from_millis(200));
                                redraw = true;
                            }
                        }
                    }
                    Ok(InternalTuiEvent::BuildUiEvent(BuildUiEvent::Cancel)) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        }

        if let Some(Err(error)) = build_result {
            self.stop();
            println!();
            let _ = ErrorPrinter::print_alien_error(&error.into_generic(), Some("BUILD FAILED"));
        } else {
            self.stop();
            println!();
        }

        Ok(())
    }

    fn input_handler_thread(tx: mpsc::Sender<InternalTuiEvent>) {
        let tick_rate = Duration::from_millis(80);
        let mut last_tick = Instant::now();

        loop {
            let timeout = tick_rate.saturating_sub(last_tick.elapsed());

            if event::poll(timeout).unwrap_or(false) {
                match event::read() {
                    Ok(event::Event::Key(key)) => {
                        if tx.send(InternalTuiEvent::Input(key)).is_err() {
                            break;
                        }
                    }
                    Ok(event::Event::Resize(_, _)) => {
                        if tx.send(InternalTuiEvent::Resize).is_err() {
                            break;
                        }
                    }
                    _ => {}
                }
            }

            if last_tick.elapsed() >= tick_rate {
                if tx.send(InternalTuiEvent::Tick).is_err() {
                    break;
                }
                last_tick = Instant::now();
            }
        }
    }

    pub fn get_state(&self) -> &BuildState {
        &self.state
    }
}

impl Drop for BuildUiComponent {
    fn drop(&mut self) {
        self.stop();
    }
}

// ── Rendering ───────────────────────────────────────────────────────────

/// Pure UI renderer
pub struct BuildUi;

impl BuildUi {
    /// Calculate the total height needed for the build UI
    pub fn calculate_height(state: &BuildState) -> u16 {
        let mut height: u16 = 0;

        // Header + empty line
        height += 2;

        // Main steps (up to 4)
        height += 4;

        // Resources section
        let resource_count = state.resources.len();
        if resource_count > 0 {
            height += 1; // Empty line before resources
            height += 1; // Section header
            height += resource_count as u16; // One line per resource
        }

        // Result section
        if let Some(ref result) = state.result {
            height += 1; // Empty line
            height += Self::result_height(result);
        }

        height.max(8).min(40)
    }

    fn result_height(_result: &BuildResult) -> u16 {
        8
    }

    pub fn draw(frame: &mut Frame, state: &BuildState) {
        let area = frame.area();

        let mut constraints = vec![
            Constraint::Length(1), // Header
            Constraint::Length(1), // Empty line
            Constraint::Length(4), // Steps
        ];

        if !state.resources.is_empty() {
            constraints.push(Constraint::Length(1)); // Empty line
            constraints.push(Constraint::Length(1 + state.resources.len() as u16));
            // Header + resources
        }

        if let Some(ref result) = state.result {
            constraints.push(Constraint::Length(1)); // Empty line
            constraints.push(Constraint::Length(Self::result_height(result)));
        }

        // Remaining space
        constraints.push(Constraint::Min(0));

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .horizontal_margin(1)
            .constraints(constraints)
            .split(area);

        let mut idx = 0;

        // Header
        Self::render_header(frame, chunks[idx], state);
        idx += 2; // skip empty line

        // Steps
        Self::render_main_steps(frame, chunks[idx], state);
        idx += 1;

        // Resources
        if !state.resources.is_empty() {
            idx += 1; // skip empty line
            Self::render_resources(frame, chunks[idx], state);
            idx += 1;
        }

        // Result
        if state.result.is_some() {
            idx += 1; // skip empty line
            Self::render_result(frame, chunks[idx], state);
        }
    }

    fn render_header(frame: &mut Frame, area: Rect, state: &BuildState) {
        widgets::render_header(
            frame,
            area,
            "Building your application for",
            &state.platform,
            Color::Rgb(34, 197, 94),
        );
    }

    fn render_main_steps(frame: &mut Frame, area: Rect, state: &BuildState) {
        let spinner_char = SPINNER_FRAMES[state.spinner_frame];
        let current_idx = state.current_step_index();
        let mut lines = Vec::new();
        let mut hit_failure = false;

        for (i, key) in STEP_ORDER.iter().enumerate() {
            if i > current_idx || hit_failure {
                break;
            }
            if let Some(step) = state.steps.get(*key) {
                if matches!(step.status, StepStatus::Failed(_)) {
                    hit_failure = true;
                }
                let (symbol, color) = widgets::get_step_status_display(&step.status, spinner_char);

                let mut spans = vec![
                    Span::styled(symbol, Style::default().fg(color)),
                    Span::raw(" "),
                    Span::raw(step.name.clone()),
                ];

                if let (StepStatus::Completed, Some(warnings)) = (&step.status, step.warning_count)
                {
                    if warnings > 0 {
                        spans.push(Span::styled(
                            format!(" ({} warnings)", warnings),
                            Style::default().fg(Color::Rgb(245, 158, 11)),
                        ));
                    }
                }

                lines.push(Line::from(spans));
            }
        }

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_resources(frame: &mut Frame, area: Rect, state: &BuildState) {
        let mut items = Vec::new();

        // Dynamic section header
        let header = state.resources_section_header();
        items.push(ListItem::new(Line::from(Span::raw(header))));

        // Sort resources by name
        let mut resources: Vec<_> = state.resources.values().collect();
        resources.sort_by(|a, b| a.name.cmp(&b.name));

        let spinner_char = SPINNER_FRAMES[state.spinner_frame];

        for resource in resources {
            items.push(ListItem::new(Self::render_resource_line(
                resource,
                spinner_char,
            )));
        }

        let list = List::new(items).block(Block::default().borders(Borders::NONE));
        frame.render_widget(list, area);
    }

    fn render_resource_line(resource: &ResourceBuildState, spinner_char: char) -> Line<'static> {
        const NAME_WIDTH: usize = 30;

        let mut spans = vec![
            Span::raw("  "),
            Span::styled(
                format!("{:<width$}", resource.name, width = NAME_WIDTH),
                Style::default().fg(Color::Cyan).bold(),
            ),
            Span::raw("  "),
        ];

        let (status_text, status_color, duration_text) = match &resource.phase {
            BuildPhase::Queued => ("Queued".to_string(), Color::Rgb(107, 114, 128), None),
            BuildPhase::Compiling { progress } => {
                let text = match progress {
                    Some(p) => format!("{} Building \u{2022} {}", spinner_char, p),
                    None => format!("{} Building...", spinner_char),
                };
                (text, Color::Rgb(245, 158, 11), None)
            }
            BuildPhase::Completed { duration } => (
                "\u{2713} Built".to_string(),
                Color::Rgb(34, 197, 94),
                Some(format!("({:.1}s)", duration)),
            ),
            BuildPhase::Failed { .. } => {
                ("\u{2717} Failed".to_string(), Color::Rgb(239, 68, 68), None)
            }
            BuildPhase::Canceled => (
                "\u{2717} Canceled".to_string(),
                Color::Rgb(156, 163, 175),
                None,
            ),
        };

        spans.push(Span::styled(status_text, Style::default().fg(status_color)));

        if let Some(dur) = duration_text {
            spans.push(Span::styled(
                format!(" {}", dur),
                Style::default().fg(Color::Rgb(156, 163, 175)),
            ));
        }

        Line::from(spans)
    }

    fn render_result(frame: &mut Frame, area: Rect, state: &BuildState) {
        if let Some(BuildResult::Success) = &state.result {
            let success_message = "\u{2728} Your application is ready for deployment!";
            let output_dir_step =
                format!(" \u{2022} Review generated files in {}", state.output_dir);
            let next_steps = vec![
                output_dir_step.as_str(),
                " \u{2022} Deploy your application: alien apply",
            ];
            widgets::render_success_box(
                frame,
                area,
                success_message,
                next_steps,
                state.elapsed(),
                Color::Rgb(34, 197, 94),
            );
        }
        // Errors are printed to terminal after TUI stops
    }
}

/// Calculate the required height for a build state (convenience function for demos)
pub fn calculate_required_height(state: &BuildState) -> u16 {
    BuildUi::calculate_height(state)
}
