//! Self-contained React-like release UI component
//!
//! This module contains a complete TUI component that handles all UI logic,
//! events, drawing, and state management for the release command.

use crate::error::{ErrorData, Result};
use crate::tui::common::{
    widgets, StepState, StepStatus, MAX_VIEWPORT_HEIGHT, MIN_VIEWPORT_HEIGHT, SPINNER_FRAMES,
};
use crate::tui::ErrorPrinter;
use alien_core::{AlienEvent, EventChange, EventState, PushProgress};
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

/// Represents the overall state of the release process
#[derive(Debug, Clone)]
pub struct ReleaseState {
    /// Main release steps and their current status
    pub steps: HashMap<String, StepState>,
    /// Resources being pushed and their progress
    pub functions: HashMap<String, FunctionPushState>,
    /// Track parent-child relationships for event hierarchy
    pub event_parents: HashMap<String, String>,
    /// Map PushingResource event IDs to resource names for direct lookup
    pub resource_event_ids: HashMap<String, String>,
    /// Current spinner frame for animations
    pub spinner_frame: usize,
    /// Release start time
    pub start_time: Instant,
    /// Release end time (if completed)
    pub end_time: Option<Instant>,
    /// Target platforms
    pub platforms: Vec<String>,
    /// Project name
    pub project_name: String,
    /// Final result
    pub result: Option<ReleaseResult>,
    /// Current step index (for ordering)
    pub current_step_index: usize,
    /// Previous calculated height for viewport resizing
    pub previous_height: u16,
}

/// State of a function being pushed
#[derive(Debug, Clone)]
pub struct FunctionPushState {
    /// Function name
    pub name: String,
    /// Current push phase
    pub phase: FunctionPushPhase,
    /// Start time
    pub start_time: Option<Instant>,
    /// End time
    pub end_time: Option<Instant>,
}

/// Current phase of function pushing
#[derive(Debug, Clone)]
pub enum FunctionPushPhase {
    Queued,
    Pushing {
        /// Push progress information
        progress: Option<PushProgress>,
    },
    Completed {
        /// Total duration in seconds
        duration: f64,
    },
    Failed {
        /// Error message
        error: String,
    },
}

/// Final release result
#[derive(Debug, Clone)]
pub enum ReleaseResult {
    Success { release_id: String },
    Failed(AlienError<GenericError>),
}

impl ReleaseState {
    /// Create a new release state
    pub fn new(platforms: Vec<String>, project_name: String) -> Self {
        let mut steps = HashMap::new();

        // Initialize main release steps
        steps.insert(
            "configuration".to_string(),
            StepState {
                name: "Load configuration".to_string(),
                status: StepStatus::Pending,
                warning_count: None,
            },
        );

        steps.insert(
            "push".to_string(),
            StepState {
                name: "Push images".to_string(),
                status: StepStatus::Pending,
                warning_count: None,
            },
        );

        steps.insert(
            "create_release".to_string(),
            StepState {
                name: "Create release".to_string(),
                status: StepStatus::Pending,
                warning_count: None,
            },
        );

        let initial_height = Self::calculate_height_for_empty_state();

        Self {
            steps,
            functions: HashMap::new(),
            event_parents: HashMap::new(),
            resource_event_ids: HashMap::new(),
            spinner_frame: 0,
            start_time: Instant::now(),
            end_time: None,
            platforms,
            project_name,
            result: None,
            current_step_index: 0,
            previous_height: initial_height,
        }
    }

    /// Calculate initial height for an empty release state
    fn calculate_height_for_empty_state() -> u16 {
        let mut height = 0u16;
        // Header + empty line
        height += 2;
        // Main steps (3 steps)
        height += 3;
        // Extra padding
        height += 2;
        // Ensure within bounds
        height.max(MIN_VIEWPORT_HEIGHT).min(MAX_VIEWPORT_HEIGHT)
    }

    /// Update spinner animation
    pub fn tick(&mut self) {
        self.spinner_frame = (self.spinner_frame + 1) % SPINNER_FRAMES.len();
    }

    /// Check if the UI height has changed and update internal tracking
    pub fn check_and_update_height(&mut self) -> Option<u16> {
        let new_height = ReleaseUi::calculate_height(self);
        let constrained_height = new_height.max(MIN_VIEWPORT_HEIGHT).min(MAX_VIEWPORT_HEIGHT);

        if constrained_height != self.previous_height {
            self.previous_height = constrained_height;
            Some(constrained_height)
        } else {
            None
        }
    }

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
                // Store parent-child relationship if parent exists
                if let Some(parent) = parent_id {
                    self.event_parents.insert(id.clone(), parent.clone());
                }

                // Track PushingResource events directly
                if let AlienEvent::PushingResource { resource_name, .. } = event {
                    self.resource_event_ids
                        .insert(id.clone(), resource_name.clone());
                }

                let parent_ref = parent_id.as_ref().map(String::as_str);
                self.handle_event_with_context(id, parent_ref, event, state);
            }
            EventChange::Updated { id, event, .. } => {
                // For updates, look up parent context
                let parent_ref = self.event_parents.get(id).cloned();
                self.handle_event_with_context(id, parent_ref.as_deref(), event, &EventState::None);
            }
            EventChange::StateChanged { id, new_state, .. } => {
                // For state changes, handle with context
                self.handle_state_change(id, new_state);
            }
        }
    }

    /// Get the resource name associated with any event ID by traversing hierarchy
    fn get_resource_for_event(&self, event_id: &str) -> Option<String> {
        if let Some(name) = self.resource_event_ids.get(event_id) {
            return Some(name.clone());
        }

        // Traverse up the parent chain to find a PushingResource event
        let mut current_id = event_id;
        for _ in 0..10 {
            if let Some(name) = self.resource_event_ids.get(current_id) {
                return Some(name.clone());
            }
            match self.event_parents.get(current_id) {
                Some(parent) => current_id = parent,
                None => break,
            }
        }

        None
    }

    /// Handle state changes for existing events
    fn handle_state_change(&mut self, event_id: &str, new_state: &EventState) {
        match new_state {
            EventState::Success => {
                // Check which step should be completing
                match self.current_step_index {
                    0 => {
                        // Configuration completing
                        if let Some(step) = self.steps.get_mut("configuration") {
                            if matches!(step.status, StepStatus::InProgress) {
                                step.status = StepStatus::Completed;
                                self.current_step_index = 1;
                            }
                        }
                    }
                    1 => {
                        // Push completing
                        if let Some(step) = self.steps.get_mut("push") {
                            if matches!(step.status, StepStatus::InProgress) {
                                step.status = StepStatus::Completed;
                                self.current_step_index = 2;
                            }
                        }
                    }
                    2 => {
                        // Create release completing
                        if let Some(step) = self.steps.get_mut("create_release") {
                            if matches!(step.status, StepStatus::InProgress) {
                                step.status = StepStatus::Completed;
                                self.current_step_index = 3;
                            }
                        }
                    }
                    _ => {}
                }
            }
            EventState::Failed { error } => {
                let error_msg = error
                    .as_ref()
                    .map(|e| e.message.clone())
                    .unwrap_or_else(|| "Unknown error".to_string());
                match self.current_step_index {
                    0 => {
                        if let Some(step) = self.steps.get_mut("configuration") {
                            if matches!(step.status, StepStatus::InProgress) {
                                step.status = StepStatus::Failed(error_msg);
                            }
                        }
                    }
                    1 => {
                        if let Some(step) = self.steps.get_mut("push") {
                            if matches!(step.status, StepStatus::InProgress) {
                                step.status = StepStatus::Failed(error_msg);
                            }
                        }
                    }
                    2 => {
                        if let Some(step) = self.steps.get_mut("create_release") {
                            if matches!(step.status, StepStatus::InProgress) {
                                step.status = StepStatus::Failed(error_msg);
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }

        // Handle resource completion when PushingResource events transition to Success
        if let Some(function_name) = self.resource_event_ids.get(event_id) {
            if let Some(func) = self.functions.get_mut(function_name) {
                match new_state {
                    EventState::Success => {
                        func.end_time = Some(Instant::now());
                        if let Some(duration) = func.duration() {
                            func.phase = FunctionPushPhase::Completed { duration };
                        }

                        // Check if all functions are complete
                        if self.all_functions_complete() {
                            if let Some(step) = self.steps.get_mut("push") {
                                self.current_step_index = 2;
                                step.status = StepStatus::Completed;
                            }
                        }
                    }
                    EventState::Failed { error } => {
                        func.end_time = Some(Instant::now());
                        func.phase = FunctionPushPhase::Failed {
                            error: error
                                .as_ref()
                                .map(|e| e.message.clone())
                                .unwrap_or_else(|| "Unknown error".to_string()),
                        };

                        if let Some(step) = self.steps.get_mut("push") {
                            step.status = StepStatus::Failed("Function push failed".to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// Update state based on an alien event with context
    fn handle_event_with_context(
        &mut self,
        event_id: &str,
        _parent_id: Option<&str>,
        event: &AlienEvent,
        state: &EventState,
    ) {
        match event {
            AlienEvent::LoadingConfiguration => {
                if let Some(step) = self.steps.get_mut("configuration") {
                    step.status = match state {
                        EventState::Started => {
                            self.current_step_index = 0;
                            StepStatus::InProgress
                        }
                        EventState::Success => {
                            self.current_step_index = 1;
                            StepStatus::Completed
                        }
                        EventState::Failed { error } => StepStatus::Failed(
                            error
                                .as_ref()
                                .map(|e| e.message.clone())
                                .unwrap_or_else(|| "Unknown error".to_string()),
                        ),
                        _ => step.status.clone(),
                    };
                }
            }
            AlienEvent::PushingStack { .. } => {
                if let Some(step) = self.steps.get_mut("push") {
                    step.status = match state {
                        EventState::Started => {
                            self.current_step_index = 1;
                            StepStatus::InProgress
                        }
                        EventState::Success => {
                            self.current_step_index = 2;
                            StepStatus::Completed
                        }
                        EventState::Failed { error } => StepStatus::Failed(
                            error
                                .as_ref()
                                .map(|e| e.message.clone())
                                .unwrap_or_else(|| "Unknown error".to_string()),
                        ),
                        _ => step.status.clone(),
                    };
                }
            }
            AlienEvent::PushingResource { resource_name, .. } => {
                match state {
                    EventState::Started => {
                        // Mark push step as in progress if not already
                        if let Some(step) = self.steps.get_mut("push") {
                            if matches!(step.status, StepStatus::Pending) {
                                self.current_step_index = 1;
                                step.status = StepStatus::InProgress;
                            }
                        }

                        // Add or update resource
                        self.functions.insert(
                            resource_name.clone(),
                            FunctionPushState {
                                name: resource_name.clone(),
                                phase: FunctionPushPhase::Pushing { progress: None },
                                start_time: Some(Instant::now()),
                                end_time: None,
                            },
                        );
                    }
                    EventState::Success => {
                        if let Some(func) = self.functions.get_mut(resource_name) {
                            func.end_time = Some(Instant::now());
                            if let Some(duration) = func.duration() {
                                func.phase = FunctionPushPhase::Completed { duration };
                            }
                        }

                        if self.all_functions_complete() {
                            if let Some(step) = self.steps.get_mut("push") {
                                self.current_step_index = 2;
                                step.status = StepStatus::Completed;
                            }
                        }
                    }
                    EventState::Failed { error } => {
                        if let Some(func) = self.functions.get_mut(resource_name) {
                            func.end_time = Some(Instant::now());
                            func.phase = FunctionPushPhase::Failed {
                                error: error
                                    .as_ref()
                                    .map(|e| e.message.clone())
                                    .unwrap_or_else(|| "Unknown error".to_string()),
                            };
                        }

                        if let Some(step) = self.steps.get_mut("push") {
                            step.status = StepStatus::Failed("Push failed".to_string());
                        }
                    }
                    _ => {}
                }
            }
            AlienEvent::PushingImage { progress, .. } => {
                // Update pushing progress for the resource associated with this event
                if let Some(resource_name) = self.get_resource_for_event(event_id) {
                    if let Some(func) = self.functions.get_mut(&resource_name) {
                        if matches!(func.phase, FunctionPushPhase::Pushing { .. }) {
                            func.phase = FunctionPushPhase::Pushing {
                                progress: progress.clone(),
                            };
                        }
                    }
                }
            }
            AlienEvent::CreatingRelease { .. } => {
                // This event indicates the final step (create release)
                if let Some(step) = self.steps.get_mut("create_release") {
                    step.status = match state {
                        EventState::Started => {
                            self.current_step_index = 2;
                            StepStatus::InProgress
                        }
                        EventState::Success => {
                            self.current_step_index = 3;
                            StepStatus::Completed
                        }
                        EventState::Failed { error } => StepStatus::Failed(
                            error
                                .as_ref()
                                .map(|e| e.message.clone())
                                .unwrap_or_else(|| "Unknown error".to_string()),
                        ),
                        _ => step.status.clone(),
                    };
                }
            }
            _ => {}
        }
    }

    /// Set the final release result
    pub fn set_result(&mut self, result: ReleaseResult) {
        self.result = Some(result);
        self.end_time = Some(Instant::now());
    }

    /// Set an error result from any AlienError type
    pub fn set_error<T>(&mut self, error: AlienError<T>)
    where
        T: alien_error::AlienErrorData + Clone + std::fmt::Debug + serde::Serialize,
    {
        self.end_time = Some(Instant::now());
        self.set_result(ReleaseResult::Failed(error.into_generic()));
    }

    /// Check if all functions are complete
    fn all_functions_complete(&self) -> bool {
        !self.functions.is_empty()
            && self.functions.values().all(|f| {
                matches!(
                    f.phase,
                    FunctionPushPhase::Completed { .. } | FunctionPushPhase::Failed { .. }
                )
            })
    }

    /// Get elapsed time
    pub fn elapsed(&self) -> Duration {
        self.end_time.unwrap_or_else(Instant::now) - self.start_time
    }
}

impl FunctionPushState {
    /// Calculate duration if both start and end times are available
    pub fn duration(&self) -> Option<f64> {
        match (self.start_time, self.end_time) {
            (Some(start), Some(end)) => Some(end.duration_since(start).as_secs_f64()),
            _ => None,
        }
    }
}

/// Props interface for the ReleaseUiComponent
#[derive(Debug, Clone)]
pub struct ReleaseUiProps {
    /// Platforms being released
    pub platforms: Vec<String>,
    /// Project name
    pub project_name: String,
    /// Optional result callback
    pub on_result: Option<fn(ReleaseResult)>,
    /// Optional cancel callback
    pub on_cancel: Option<fn()>,
}

/// Events that can be sent to the ReleaseUiComponent
#[derive(Debug)]
pub enum ReleaseUiEvent {
    /// Alien event change from the release system
    AlienEventChange(EventChange),
    /// Release finished with result
    ReleaseFinished(std::result::Result<ReleaseResult, AlienError<crate::error::ErrorData>>),
    /// Cancel the release
    Cancel,
}

/// Internal TUI events
#[derive(Debug)]
enum InternalTuiEvent {
    Input(event::KeyEvent),
    Tick,
    Resize,
    ReleaseUiEvent(ReleaseUiEvent),
}

/// Self-contained React-like release UI component
pub struct ReleaseUiComponent {
    state: ReleaseState,
    terminal: Option<Terminal<CrosstermBackend<std::io::Stdout>>>,
    event_tx: Option<mpsc::Sender<InternalTuiEvent>>,
    event_rx: Option<mpsc::Receiver<InternalTuiEvent>>,
    running: bool,
}

impl ReleaseUiComponent {
    /// Create a new ReleaseUiComponent with props
    pub fn new(props: ReleaseUiProps) -> Self {
        let state = ReleaseState::new(props.platforms, props.project_name);

        Self {
            state,
            terminal: None,
            event_tx: None,
            event_rx: None,
            running: false,
        }
    }

    /// Initialize the TUI terminal and start the component
    pub fn start(&mut self) -> Result<mpsc::Sender<ReleaseUiEvent>> {
        if self.running {
            return Err(AlienError::new(ErrorData::TuiOperationFailed {
                message: "ReleaseUiComponent is already running".to_string(),
            }));
        }

        // Initialize terminal
        let terminal = ratatui::init_with_options(TerminalOptions {
            viewport: Viewport::Inline(MIN_VIEWPORT_HEIGHT),
        });
        self.terminal = Some(terminal);

        // Set up event channels
        let (internal_tx, internal_rx) = mpsc::channel();
        let (external_tx, external_rx) = mpsc::channel();

        self.event_rx = Some(internal_rx);
        self.running = true;

        // Start input handler
        let input_tx = internal_tx.clone();
        thread::spawn(move || {
            Self::input_handler_thread(input_tx);
        });

        // Start external event forwarder
        let forward_tx = internal_tx.clone();
        thread::spawn(move || {
            while let Ok(event) = external_rx.recv() {
                if forward_tx
                    .send(InternalTuiEvent::ReleaseUiEvent(event))
                    .is_err()
                {
                    break;
                }
            }
        });

        self.event_tx = Some(internal_tx);

        Ok(external_tx)
    }

    /// Stop the component and restore terminal
    pub fn stop(&mut self) {
        self.running = false;
        if self.terminal.is_some() {
            ratatui::restore();
            self.terminal = None;
        }

        // Ensure terminal is flushed
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
    }

    /// Run the main event loop - this blocks until the component is stopped
    pub fn run_event_loop(&mut self) -> Result<()> {
        if !self.running || self.terminal.is_none() || self.event_rx.is_none() {
            return Err(AlienError::new(ErrorData::TuiOperationFailed {
                message: "ReleaseUiComponent not properly initialized".to_string(),
            }));
        }

        let mut redraw = true;
        let mut release_result: Option<
            std::result::Result<ReleaseResult, AlienError<crate::error::ErrorData>>,
        > = None;
        let mut show_result_until: Option<Instant> = None;
        let mut stop_animation = false;

        while self.running {
            // Draw if needed
            if redraw {
                if let Some(ref mut terminal) = self.terminal {
                    let _ = terminal.draw(|frame| {
                        ReleaseUi::draw(frame, &self.state);
                    });
                }
                redraw = false;
            }

            // Check if we should exit after showing results
            if let Some(until) = show_result_until {
                if Instant::now() >= until {
                    break;
                }
            }

            // Handle events
            if let Some(ref rx) = self.event_rx {
                match rx.recv_timeout(Duration::from_millis(80)) {
                    Ok(InternalTuiEvent::Input(key)) => match key.code {
                        event::KeyCode::Char('c')
                            if key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            break;
                        }
                        _ => {}
                    },
                    Ok(InternalTuiEvent::Tick) => {
                        if !stop_animation {
                            self.state.tick();
                            redraw = true;
                        }
                    }
                    Ok(InternalTuiEvent::Resize) => {
                        redraw = true;
                    }
                    Ok(InternalTuiEvent::ReleaseUiEvent(ReleaseUiEvent::AlienEventChange(
                        event_change,
                    ))) => {
                        self.state.handle_event_change(&event_change);

                        // Check if height changed and resize viewport if needed
                        if let Some(new_height) = self.state.check_and_update_height() {
                            if let Some(ref mut terminal) = self.terminal {
                                let _ = terminal.set_viewport_height(new_height);
                            }
                        }

                        redraw = true;
                    }
                    Ok(InternalTuiEvent::ReleaseUiEvent(ReleaseUiEvent::ReleaseFinished(
                        result,
                    ))) => {
                        release_result = Some(result.clone());
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
                    Ok(InternalTuiEvent::ReleaseUiEvent(ReleaseUiEvent::Cancel)) => {
                        break;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        // Timeout is normal
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        break;
                    }
                }
            }
        }

        // Handle final result display
        if let Some(Err(error)) = release_result {
            self.stop();
            let _ = ErrorPrinter::print_alien_error(&error.into_generic(), Some("RELEASE FAILED"));
        } else {
            self.stop();
        }

        Ok(())
    }

    /// Input handler thread
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

    /// Get the current state (for testing/inspection)
    pub fn get_state(&self) -> &ReleaseState {
        &self.state
    }
}

impl Drop for ReleaseUiComponent {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Pure UI renderer
pub struct ReleaseUi;

impl ReleaseUi {
    /// Calculate the total height needed for the release UI
    pub fn calculate_height(state: &ReleaseState) -> u16 {
        let mut height = 0u16;

        // Header
        height += 1;

        // Empty line after header
        height += 1;

        // Main steps (always show these 3)
        height += 3;

        // Functions section if we have any functions
        let function_count = state.functions.len();
        if function_count > 0 {
            height += 1; // Empty line before functions
            height += 1; // Functions header
            height += function_count as u16; // One line per function
        }

        // Result section when complete
        if let Some(ref result) = state.result {
            height += 1; // Empty line
            height += Self::calculate_result_height(result); // Dynamic result height
        }

        // Add padding and ensure bounds
        height += 2;
        height.max(MIN_VIEWPORT_HEIGHT).min(MAX_VIEWPORT_HEIGHT)
    }

    /// Calculate the height needed for a result section
    fn calculate_result_height(_result: &ReleaseResult) -> u16 {
        8
    }

    /// Render the release UI
    pub fn draw(frame: &mut Frame, state: &ReleaseState) {
        let area = frame.area();

        let mut constraints = vec![
            Constraint::Length(1), // Header
            Constraint::Length(1), // Empty line
            Constraint::Length(3), // Main steps (3 lines)
        ];

        // Functions section
        if !state.functions.is_empty() {
            constraints.push(Constraint::Length(1)); // Empty line
            constraints.push(Constraint::Length(1 + state.functions.len() as u16));
            // Header + functions
        }

        // Result section
        if let Some(ref result) = state.result {
            if !state.functions.is_empty() {
                constraints.push(Constraint::Length(1)); // Empty line
            } else {
                constraints.push(Constraint::Length(1)); // Empty line
            }

            let result_height = Self::calculate_result_height(result);
            constraints.push(Constraint::Length(result_height));
        } else {
            constraints.push(Constraint::Min(0));
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints(constraints)
            .split(area);

        let mut chunk_index = 0;

        // Header
        Self::render_header(frame, chunks[chunk_index], state);
        chunk_index += 2; // Skip empty line

        // Main steps
        Self::render_main_steps(frame, chunks[chunk_index], state);
        chunk_index += 1;

        // Functions (if any)
        if !state.functions.is_empty() {
            chunk_index += 1; // Skip empty line
            Self::render_functions(frame, chunks[chunk_index], state);
            chunk_index += 1;
        }

        // Result (if complete)
        if state.result.is_some() {
            chunk_index += 1; // Skip empty line
            Self::render_result(frame, chunks[chunk_index], state);
        }
    }

    fn render_header(frame: &mut Frame, area: Rect, state: &ReleaseState) {
        let platforms_display = state.platforms.join(", ").to_uppercase();
        let title = format!("Releasing {} to", state.project_name);
        let color = Color::Rgb(139, 92, 246); // Purple

        widgets::render_header(frame, area, &title, &platforms_display, color);
    }

    fn render_main_steps(frame: &mut Frame, area: Rect, state: &ReleaseState) {
        let spinner_char = SPINNER_FRAMES[state.spinner_frame];

        let step_order = ["configuration", "push", "create_release"];
        let mut lines = Vec::new();

        let mut should_stop = false;
        for (index, step_id) in step_order.iter().enumerate() {
            if index <= state.current_step_index && !should_stop {
                if let Some(step) = state.steps.get(*step_id) {
                    if matches!(step.status, StepStatus::Failed(_)) {
                        should_stop = true;
                    }
                    let (symbol, color) =
                        widgets::get_step_status_display(&step.status, spinner_char);

                    let spans = vec![
                        Span::styled(symbol.clone(), Style::default().fg(color)),
                        Span::raw(" "),
                        Span::raw(step.name.clone()),
                    ];

                    lines.push(Line::from(spans));
                }
            }
        }

        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, area);
    }

    fn render_functions(frame: &mut Frame, area: Rect, state: &ReleaseState) {
        let mut items = Vec::new();

        // Header
        items.push(ListItem::new(Line::from(vec![
            Span::raw("Resources ("),
            Span::styled(state.functions.len().to_string(), Style::default().bold()),
            Span::raw(" total):"),
        ])));

        // Sort functions by name
        let mut functions: Vec<_> = state.functions.values().collect();
        functions.sort_by(|a, b| a.name.cmp(&b.name));

        let spinner_char = SPINNER_FRAMES[state.spinner_frame];

        for function in functions {
            let line = Self::render_function_line(function, spinner_char);
            items.push(ListItem::new(line));
        }

        let list = List::new(items).block(Block::default().borders(Borders::NONE));
        frame.render_widget(list, area);
    }

    fn render_function_line(function: &FunctionPushState, spinner_char: char) -> Line<'_> {
        const FUNCTION_NAME_WIDTH: usize = 25;

        let mut spans = vec![
            Span::raw("  "),
            Span::styled(
                format!("{:<width$}", function.name, width = FUNCTION_NAME_WIDTH),
                Style::default().fg(Color::Cyan).bold(),
            ),
            Span::raw("     "),
        ];

        let (status_text, status_color, progress_bar) = match &function.phase {
            FunctionPushPhase::Queued => ("Queued".to_string(), Color::Rgb(107, 114, 128), None),
            FunctionPushPhase::Pushing { progress } => {
                let (text, bar) = if let Some(p) = progress {
                    if p.total_bytes > 0 || p.total_layers > 0 {
                        let progress_ratio = if p.total_bytes > 0 {
                            p.bytes_uploaded as f64 / p.total_bytes as f64
                        } else {
                            p.layers_uploaded as f64 / p.total_layers as f64
                        };

                        let filled_chars = (progress_ratio * 20.0).round() as usize;
                        let filled_chars = filled_chars.min(20);
                        let bar_str = "█".repeat(filled_chars) + &"░".repeat(20 - filled_chars);

                        let text = if p.total_bytes > 0 && p.bytes_uploaded > 0 {
                            let uploaded_mb = p.bytes_uploaded as f64 / (1024.0 * 1024.0);
                            let total_mb = p.total_bytes as f64 / (1024.0 * 1024.0);
                            let percentage = (progress_ratio * 100.0).round() as u8;
                            format!(
                                "{} Pushing {:.1}/{:.1} MB ({}%)",
                                spinner_char, uploaded_mb, total_mb, percentage
                            )
                        } else if p.total_layers > 0 {
                            let percentage = (progress_ratio * 100.0).round() as u8;
                            format!(
                                "{} Pushing layer {}/{} ({}%)",
                                spinner_char, p.layers_uploaded, p.total_layers, percentage
                            )
                        } else {
                            format!("{} Pushing...", spinner_char)
                        };

                        (text, Some(bar_str))
                    } else {
                        (format!("{} Pushing...", spinner_char), None)
                    }
                } else {
                    (format!("{} Pushing...", spinner_char), None)
                };
                (text, Color::Rgb(245, 158, 11), bar)
            }
            FunctionPushPhase::Completed { duration } => {
                let text = format!("✓ Pushed ({:.1}s)", duration);
                let bar = "████████████████████".to_string();
                (text, Color::Rgb(34, 197, 94), Some(bar))
            }
            FunctionPushPhase::Failed { error: _ } => {
                ("✗ Failed".to_string(), Color::Rgb(239, 68, 68), None)
            }
        };

        spans.push(Span::styled(
            format!("{:<40}", status_text),
            Style::default().fg(status_color),
        ));

        // Add progress bar if present
        if let Some(bar) = progress_bar {
            spans.extend(vec![
                Span::raw("["),
                Span::styled(bar, Style::default().fg(status_color)),
                Span::raw("]"),
            ]);
        }

        Line::from(spans)
    }

    fn render_result(frame: &mut Frame, area: Rect, state: &ReleaseState) {
        match &state.result {
            Some(ReleaseResult::Success { release_id }) => {
                Self::render_success_box(frame, area, state, release_id);
            }
            Some(ReleaseResult::Failed(_error)) => {
                // Error will be shown in terminal
            }
            None => {}
        }
    }

    fn render_success_box(frame: &mut Frame, area: Rect, state: &ReleaseState, release_id: &str) {
        let success_message = format!("✨ Release {} created successfully!", release_id);
        let next_steps = vec![
            " • Deploy: alien deploy",
            " • View in dashboard: https://alien.dev/releases",
        ];
        let color = Color::Rgb(139, 92, 246); // Purple

        widgets::render_success_box(
            frame,
            area,
            &success_message,
            next_steps,
            state.elapsed(),
            color,
        );
    }
}
