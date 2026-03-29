# The Alien CLI

`alien` is a plain CLI first. There is no dashboard mode and no TUI fallback. Human terminals get readable progress and small setup prompts where helpful; automation gets flags, `--json`, and fast failures.

## Mode Resolution

The CLI resolves its target in this order:

```text
if command is `alien dev ...`:
    local dev mode
elif ALIEN_MANAGER_URL is set:
    standalone manager mode
else:
    platform mode
```

## Command Categories

The command tree has four categories.

### 1. Offline / build commands

These run on your machine and do not need a manager.

| Command | Purpose |
|---|---|
| `alien build --platform <platform>` | Build the stack into `.alien/build/<platform>` |

### 2. Top-level manager commands

These talk to a non-local manager API.

| Command | Purpose |
|---|---|
| `alien release` | Push images and create a release |
| `alien deploy` | Create or update a deployment |
| `alien destroy` | Destroy a deployment |
| `alien deployments ...` | List and inspect deployments |
| `alien onboard` | Create a deployment group / onboarding flow |
| `alien whoami` | Show manager identity information |

In platform mode these commands resolve the right manager through the platform using workspace, project, and target platform. In standalone mode they talk directly to `ALIEN_MANAGER_URL`.

### 3. Local commands under `alien dev`

These target the embedded local manager only.

| Command | Purpose |
|---|---|
| `alien dev` | Start local manager, build, release, deploy, and stay attached |
| `alien dev server` | Start only the local manager |
| `alien dev release` | Create a local release |
| `alien dev deploy` | Create or update a local deployment |
| `alien dev destroy` | Destroy a local deployment |
| `alien dev deployments ...` | Inspect local deployments |
| `alien dev vault ...` | Manage local vault state |
| `alien dev whoami` | Show local manager identity |

Local behavior stays under `alien dev ...`. Top-level manager commands do not grow a separate `--local` mode.

### 4. Platform-only commands

These operate on platform entities, not manager APIs.

| Command | Purpose |
|---|---|
| `alien login` | Authenticate and set a default workspace |
| `alien logout` | Clear stored credentials |
| `alien workspaces ls` | List workspaces |
| `alien workspaces set` | Set the active workspace |
| `alien projects ls` | List projects in the active workspace |
| `alien link` | Link the current directory to a project |
| `alien unlink` | Remove the local project link |
| `alien manager ...` | Manage private managers through the platform API |

## Platform Mode

Platform mode is the default when `ALIEN_MANAGER_URL` is not set.

```bash
alien login
alien link --project my-app
alien build --platform aws
alien release
alien deployments ls
```

Key rules:

- authentication is OAuth or `ALIEN_API_KEY`
- workspace comes from `--workspace`, `ALIEN_WORKSPACE`, login profile, or an interactive bootstrap prompt
- project comes from `--project`, `.alien/project.json`, or an interactive bootstrap prompt
- manager-targeted commands do not require a manual manager URL in normal platform usage

## Standalone Mode

Standalone mode talks directly to one manager.

```bash
export ALIEN_MANAGER_URL=http://localhost:8080
export ALIEN_API_KEY=ax_admin_...

alien build --platform local
alien release
alien onboard --name customer-a
```

Key differences from platform mode:

- `ALIEN_API_KEY` is required
- there are no workspaces or projects
- platform-only commands such as `login`, `workspaces`, `projects`, and `link` do not apply

## Local Dev Mode

`alien dev` is the explicit local namespace.

```bash
cd my-app
alien dev
```

Bare `alien dev`:

1. starts the local manager
2. builds the app for the local platform unless `--skip-build`
3. creates a local release
4. creates or updates the initial local deployment
5. waits until the deployment is ready
6. prints URLs and next-step commands

`alien dev server` starts only the local manager.

## Interactive Bootstrap Rules

Interactive prompts are allowed only as bootstrap help for humans in a real terminal.

Examples:

- `alien login` can ask for a workspace
- `alien workspaces set` can offer a workspace selector
- `alien link` can help choose or create a project
- `alien release` can bootstrap missing workspace/project context in a TTY

Rules:

- there is always a complete flag-based path
- `--json` never prompts
- non-interactive execution never depends on prompts
- prompts are plain terminal prompts, not full-screen interfaces

When automation is missing context, the CLI should fail with an actionable message such as “run `alien link --project <name>`” or “run `alien workspaces set <name>`”.

## Machine-Readable Output

Major commands support `--json` and treat it as a strict machine contract.

Examples:

```bash
alien build --json
alien release --json --yes
alien whoami --json
alien projects ls --json
alien manager ls --json
```

`alien dev` also exposes a machine interface for tooling through `--status-file`:

```bash
alien dev --port 9090 --status-file .alien/dev-status.json
```

That file is written as JSON using the shared `DevStatus` type from `@alienplatform/core`.

## Configuration

### Platform mode

Resolution order:

1. CLI flags
2. environment variables
3. `.alien/project.json`
4. login profile
5. built-in defaults

Important variables:

```bash
ALIEN_BASE_URL=https://api.alien.dev
ALIEN_API_KEY=ax_...
ALIEN_WORKSPACE=my-workspace
```

### Standalone mode

Required:

```bash
ALIEN_MANAGER_URL=http://localhost:8080
ALIEN_API_KEY=ax_admin_...
```

### Local dev mode

No auth is required. The local manager is addressed through `alien dev ...` and defaults to `http://localhost:9090`.

## Common Flows

### Human first run on the platform

```bash
alien login
alien link
alien build --platform aws
alien release
```

### CI / agent flow on the platform

```bash
alien login --workspace my-workspace --json
alien link --project my-app --json
alien build --platform aws --json
alien release --project my-app --yes --json
```

### Local development

```bash
alien dev
alien dev deployments ls
alien dev release
```

### Standalone manager

```bash
export ALIEN_MANAGER_URL=http://localhost:8080
export ALIEN_API_KEY=ax_admin_...

alien build --platform local
alien release
alien deployments ls
```
