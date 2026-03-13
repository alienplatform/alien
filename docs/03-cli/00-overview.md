# The Alien CLI

The primary tool for building and managing Alien applications. One CLI, three modes of operation.

## Mode Resolution

The CLI determines which mode to use based on how it's invoked:

```
if command is `alien dev ...`:
    → Dev mode
elif ALIEN_SERVER is set (or --server flag):
    → Self-hosted mode
else:
    → Platform mode
```

### Platform mode (default)

The CLI talks to the Alien platform. The platform handles routing requests to the appropriate managed server, workspace/project scoping, OAuth, and artifact registry management.

```bash
alien login                          # OAuth → selects workspace
alien link --project my-app          # Links directory to project
alien build --platform aws           # Compile locally
alien release                        # Push images, create release
alien deployments ls                 # List deployments
```

No server URLs, no manual configuration. Authentication via OAuth (`alien login`) or API key (`ALIEN_API_KEY`).

For custom platform deployments (e.g., an enterprise running their own Alien platform), set `ALIEN_BASE_URL`:

```bash
export ALIEN_BASE_URL=https://api.acme-alien.com
export ALIEN_API_KEY=ax_...
```

This is still platform mode — it has workspaces, projects, OAuth — just at a different URL.

### Self-hosted mode

The CLI talks directly to a standalone alien-manager. Set `ALIEN_SERVER` (or use `--server`):

```bash
export ALIEN_SERVER=http://my-server:8080
export ALIEN_API_KEY=ax_admin_...

alien build --platform aws
alien release
alien deployment-groups create --name production
alien deploy --token ax_dg_... --platform aws --name production
alien deployments ls
```

**Key differences from platform mode:**
- `ALIEN_API_KEY` is required — no OAuth against a bare server
- No workspaces, no projects — single-project server
- Platform-only commands are unavailable (`login`, `workspace`, `projects`, `link`, `packages`)

### Dev mode

`alien dev` starts a local alien-manager, builds your stack, and deploys everything on your machine. No cloud, no credentials, no internet.

```bash
cd my-app
alien dev
```

All dev commands route to `localhost:9090`. Workspace and project are constants (`local-dev`). See [Local Development](02-manager/08-local-development.md).

### Why two URL variables?

`ALIEN_BASE_URL` and `ALIEN_SERVER` serve different purposes:

- **`ALIEN_BASE_URL`** = "where is the platform?" — points at the Alien platform API (default: `api.alien.dev`). The platform routes requests, manages workspaces and projects, handles OAuth. Setting a custom URL means you're using a different platform deployment, not a bare server.

- **`ALIEN_SERVER`** = "where is my alien-manager?" — points directly at a standalone alien-manager. No platform in the middle. No workspaces, no OAuth, no routing. Just a server with an API key.

The presence of `ALIEN_SERVER` is what switches the CLI to self-hosted mode.

## Commands

### Offline

| Command | Description |
|---------|-------------|
| `alien build --platform <platform>` | Compile `alien.config.ts` into OCI image tarballs |

`alien build` runs entirely on your machine. No server involved.

### Server commands

These work against any alien-manager — platform-managed, self-hosted, or local dev. In platform mode, they route through the platform API transparently.

| Command | Description |
|---------|-------------|
| `alien release` | Push images to artifact registry + create release |
| `alien deploy` | Provision a deployment in a remote environment |
| `alien destroy` | Teardown a deployment's cloud resources |
| `alien deployments ls` | List deployments |
| `alien deployments get <id>` | Get deployment details |
| `alien deployments retry <id>` | Retry a failed deployment |
| `alien deployments redeploy <id>` | Trigger redeployment with the same release |
| `alien command invoke` | Execute a remote command on a deployment |
| `alien deployment-groups create` | Create a deployment group |
| `alien onboard` | Create deployment group + generate deployment link |
| `alien whoami` | Check authenticated identity |

### Platform-only commands

These only work in platform mode. They don't exist on standalone alien-managers.

| Command | Description |
|---------|-------------|
| `alien deployments pin <id> [release]` | Pin deployment to specific release (omit release to unpin) |
| `alien login` | OAuth authentication + workspace selection |
| `alien logout` | Clear stored credentials |
| `alien workspace ls` | List workspaces |
| `alien workspace set` | Set default workspace |
| `alien projects ls` | List projects in workspace |
| `alien link` | Link current directory to a platform project |
| `alien unlink` | Remove project link |
| `alien packages ls` | List packages in registry |

### Dev mode commands

Prefix any server command with `dev` to run it against the local dev server:

```bash
alien dev deployments ls                    # List local deployments
alien dev release                           # Create local release
alien dev deploy                            # Deploy locally
alien dev whoami                            # Show dev server identity
alien dev vault set <vault> <key> <value>   # Set dev vault secret
alien dev server                            # Start dev server only (no TUI)
```

Dev mode uses the same command implementations — only the target changes.

## Configuration

### Authentication

**Platform mode** — two methods:
1. **OAuth** — `alien login` opens the browser, runs PKCE flow, stores tokens in the system keyring. Tokens auto-refresh.
2. **API key** — set `ALIEN_API_KEY` env var. Used in CI/CD (GitHub Actions).

**Self-hosted mode** — `ALIEN_API_KEY` required. The admin token is printed on first server startup.

**Dev mode** — no authentication. The dev server accepts all requests.

### Project linking

`alien link` creates `.alien/project.json` in the current directory:

```json
{
  "workspace": "my-workspace",
  "project_id": "prj_xxx",
  "project_name": "my-app"
}
```

Commands that need a project (like `alien release`) check for this file first. If not found, they prompt interactively or use the `--project` flag.

Project linking is platform-only. In self-hosted mode, there's one project — no linking needed. In dev mode, the project is always `local-dev`.

### Configuration hierarchy

**Platform mode** — resolution order (first match wins):

1. **CLI flags** — `--api-key`, `--workspace`, `--project`
2. **Environment variables** — `ALIEN_BASE_URL`, `ALIEN_API_KEY`, `ALIEN_WORKSPACE`
3. **Project link** — `.alien/project.json` (workspace and project)
4. **Profile** — `~/.config/alien/profile.json` (default workspace from `alien login`)
5. **Defaults** — `https://api.alien.dev`

**Self-hosted mode** — resolution order:

1. **CLI flags** — `--server`, `--api-key`
2. **Environment variables** — `ALIEN_SERVER`, `ALIEN_API_KEY`

No project link, no profile, no defaults. `ALIEN_API_KEY` is required.

**Dev mode** — no resolution needed. Target is always `localhost:{port}`, no auth, workspace/project are `local-dev`.

### Configuration files

| File | Created by | Contents |
|------|-----------|----------|
| `~/.config/alien/profile.json` | `alien login` | Default workspace name |
| `.alien/project.json` | `alien link` | Project link (workspace, project ID, name) |
| `.alien/dev.db` | `alien dev` | Dev server SQLite database |
| `.alien/build/` | `alien build` | Built OCI tarballs |
| System keyring | `alien login` | OAuth access + refresh tokens |

### Environment variables

```bash
# Platform mode
ALIEN_BASE_URL=https://api.alien.dev    # Platform API URL (default)
ALIEN_API_KEY=ax_...                     # API key for auth (skips OAuth)
ALIEN_WORKSPACE=my-workspace             # Default workspace

# Self-hosted mode
ALIEN_SERVER=http://my-server:8080       # Self-hosted server URL (triggers self-hosted mode)
ALIEN_API_KEY=ax_admin_...               # API key (required in self-hosted mode)

# Debugging
ALIEN_LOG=debug                          # Log level
ALIEN_LOG_FILE=/tmp/alien.log            # Log to file
```

## The Release Flow

`alien release` pushes built images and creates a release record:

1. **Resolve project** — from `--project` flag, `.alien/project.json`, or interactive prompt (platform mode only)
2. **Fetch registry config** — calls the server to get artifact registry type, endpoint, and push credentials
3. **Verify build** — checks `.alien/build/{platform}/` for built OCI tarballs
4. **Push images** — pushes each resource's image to the registry
5. **Create release** — `POST /v1/releases` with stack JSON containing image URIs
6. **Deployments auto-update** — server sets `desired_release_id` on eligible deployments

The developer doesn't specify a repository. The server provides the registry configuration — the CLI just pushes where the server tells it to.

## The Deploy Flow

`alien deploy` provisions a deployment in a remote environment:

1. **Authenticate** — validates the deployment group token via `/v1/whoami`
2. **Create deployment** — registers with the server, gets a deployment ID and deployment token
3. **Sync loop** — calls `acquire` → runs `alien-deployment::step()` locally → calls `reconcile`
4. **Track locally** — stores deployment ID and token in the system keyring for future deploys

After initial setup, the admin's machine drives provisioning using their own cloud credentials. alien-manager records the result. Subsequent updates are handled by the deployment loop (push) or Operator (pull).

## The TUI

Running `alien` or `alien dev` without a subcommand launches the Terminal UI.

**Platform TUI** (`alien`):
- Shows deployments, releases, commands for the linked project
- Read-only monitoring — deployments are managed via `alien deploy` or the dashboard

**Dev TUI** (`alien dev`):
- Shows local deployments and streams logs in real time
- Press `B` to rebuild after code changes
- Includes build status and rebuild channels

Both TUIs use the same component architecture:
- **State** — plain data structs, no async
- **Views** — pure render functions (Ratatui)
- **Services** — SDK calls that return state types
- **Controller** — orchestrates state updates from user actions and service responses

## CI/CD Integration

The same commands work in GitHub Actions:

```yaml
- name: Build
  run: alien build --platform aws --no-tui

- name: Release
  run: alien release --yes --project my-project
```

`--no-tui` disables the terminal UI for CI. `--yes` skips confirmation prompts. Authenticate with `ALIEN_API_KEY` environment variable (no OAuth needed).

## Crate Location

`alien/crates/alien-cli/`

Dependencies: `alien-build`, `alien-deployment`, `alien-manager`, `alien-client-sdk`.
