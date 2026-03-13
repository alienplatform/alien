# alien-cli Guidelines

## TUI Architecture

The TUI follows a **pure state + pure views** architecture:

```
┌─────────────────────────────────────────────────────────┐
│                    AppController                         │
│  - Handles Actions from views                           │
│  - Updates state                                        │
│  - Calls services                                       │
└─────────────────────────────────────────────────────────┘
         │                              │
         ▼                              ▼
┌─────────────────┐          ┌─────────────────────────────┐
│  AppViewState   │          │        AppServices          │
│  - Pure data    │          │  - SDK calls                │
│  - No async     │          │  - Converts to state types  │
│  - No SDK types │          └─────────────────────────────┘
└─────────────────┘
         │
         ▼
┌─────────────────────────────────────────────────────────┐
│                      Views                               │
│  - render(state) -> Frame                               │
│  - handle_key(key, state) -> Action                     │
│  - Pure functions, no side effects                      │
└─────────────────────────────────────────────────────────┘
```

### Key Principles

1. **TUI is just a view** - It observes and displays state. It never triggers deployments or business logic.

2. **Views don't own state** - They receive state as parameters and return Actions.

3. **State types are decoupled from SDK** - `DeploymentItem`, `LogLine`, etc. are plain data with display-ready strings.

4. **Logs are global** - `AppViewState.logs` accumulates all logs. Views filter by deployment_id.

5. **Resources come from polling** - The runtime polls the API every 2 seconds for fresh data.

### State Types (`tui/state/`)

```rust
// Pure data, no SDK dependencies
pub struct DeploymentItem {
    pub id: String,
    pub name: String,
    pub status: DeploymentStatus,  // Our enum, not SDK's
}

// Global log buffer
pub struct AppViewState {
    pub logs: VecDeque<LogLine>,  // All logs, filtered per-view
    pub deployments: ListState<DeploymentItem>,
    // ...
}
```

### Views (`tui/views/`)

```rust
// Pure render function
pub fn render(frame: &mut Frame, area: Rect, state: &ListState<DeploymentItem>) {
    // No async, no SDK calls, just rendering
}

// Pure key handler
pub fn handle_key(key: KeyEvent, state: &mut ListState<DeploymentItem>) -> Action {
    match key.code {
        KeyCode::Enter => Action::NavigateToDeployment(state.selected_id()),
        KeyCode::Char('n') => Action::OpenNewDeploymentDialog,
        _ => Action::None,
    }
}
```

### Services (`tui/services/`)

```rust
// Encapsulates SDK calls, converts to state types
impl DeploymentsService {
    pub async fn list(&self) -> Result<Vec<DeploymentItem>, String> {
        let response = self.sdk.list_deployments().send().await?;
        Ok(response.items.into_iter().map(deployment_from_api).collect())
    }
}
```

---

## Storybook

Test views in isolation with mock data. Located in `src/storybook/tui/`.

### Running Demos

```bash
cargo run --bin alien-cli-storybook -- tui deployments empty
cargo run --bin alien-cli-storybook -- tui deployments many-items
cargo run --bin alien-cli-storybook -- tui deployment-detail running
cargo run --bin alien-cli-storybook -- tui deployment-detail many-logs
```

### Creating Demos

```rust
// In storybook/tui/demos/deployments_list.rs
pub enum DeploymentsDemo {
    Empty,      // No deployments
    Loading,    // Loading state
    Error,      // Error state
    ManyItems,  // Scrolling, selection
}
```

Each demo constructs an `AppViewState` with specific data and runs the view.

---

## Debugging

TUI commands disable console logging. Use file logging:

```bash
# Enable file logging
ALIEN_LOG=debug ALIEN_LOG_FILE=/tmp/alien.log alien dev

# In another terminal
tail -f /tmp/alien.log
```

### Log Levels

```bash
ALIEN_LOG=debug                              # Everything
ALIEN_LOG="alien_cli=trace,alien_core=warn"  # Specific components
```

### Adding Logs

```rust
use tracing::{debug, info, warn, error};

debug!(deployment_id = %id, "Loading deployment");
info!("Server listening on port {}", port);
error!(%reason, "Failed to connect");
```

---

## Error Handling

Use `alien-error` for structured errors. See `crates/AGENTS.md` for full guide.

```rust
// New error
Err(AlienError::new(ErrorData::TuiOperationFailed { message: "..." }))

// Wrap error with context
operation().await.context(ErrorData::TuiOperationFailed { ... })?;

// Third-party errors
io_op().into_alien_error().context(ErrorData::TuiOperationFailed { ... })?;
```

---

## Common Patterns

### Adding a New View

1. **State** (`tui/state/`): Define `XxxItem` with display-ready fields
2. **Service** (`tui/services/`): Add SDK calls that return state types
3. **View** (`tui/views/`): Implement `render`, `handle_key`, `keybinds`
4. **Wire up**: Add to `AppViewState`, `AppController`, and routing in `runtime.rs`
5. **Storybook**: Add demo scenarios

### Adding a New Action

1. Add variant to `Action` enum in `tui/state/app.rs`
2. Handle in `AppController::handle_action`
3. Return from view's `handle_key`

### Refreshing Data

Data refreshes automatically via `POLL_INTERVAL` (2 seconds). For manual refresh:

```rust
KeyCode::Char('r') => Action::Refresh,
```
