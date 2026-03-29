# Local Development

`alien dev` is the product-facing local workflow. It starts a local manager, builds your app for the local platform, creates a local release, creates or updates the initial deployment, and then stays attached so the session remains alive.

```bash
cd my-app
alien dev
```

There is no local TUI. The command uses plain terminal output for humans and an explicit status-file contract for tooling.

## Bare `alien dev`

The default flow is:

1. start the local manager on `http://localhost:9090`
2. ensure local bootstrap state exists under `.alien/`
3. build the current project for the local platform unless `--skip-build`
4. create a local release
5. create or update the initial deployment
6. wait for the deployment to become ready
7. print the release ID, deployment ID, commands URL, resource URLs, and suggested next commands
8. remain running until interrupted with `Ctrl+C`

Example:

```bash
alien dev --port 9090 --deployment-name default
```

Useful flags:

| Flag | Purpose |
|---|---|
| `--port <N>` | Choose the local manager port |
| `--config <path>` | Use a specific `alien.ts` file |
| `--skip-build` | Reuse existing local build artifacts |
| `--status-file <path>` | Write machine-readable session status |
| `--deployment-name <name>` | Choose the initial deployment name |
| `--env KEY=VALUE[:targets]` | Add plain environment variables |
| `--secret KEY=VALUE[:targets]` | Add secret environment variables |

## `alien dev server`

`alien dev server` starts only the local manager.

```bash
alien dev server --port 9090
```

Use this when you want to debug the local manager itself or drive it with separate local commands such as:

```bash
alien dev deployments ls
alien dev release
alien dev deploy --name preview
```

## Machine Interface

Tooling must not scrape human log lines. The supported machine channel is `--status-file`.

```bash
alien dev --status-file .alien/dev-status.json
```

The file is updated as JSON using the shared `DevStatus` contract from `alien-core` / `@alienplatform/core`.

The status file includes:

- overall session status
- manager API URL
- agent/deployment IDs
- commands URL
- public resource URLs when available
- error information if startup fails

Typical lifecycle states:

- `initializing`
- `ready`
- `error`
- `shuttingDown`

This is the contract used by `@alienplatform/testing` local mode.

## Local Namespace

All local manager commands live under `alien dev ...`.

Examples:

```bash
alien dev deployments ls
alien dev whoami
alien dev release
alien dev deploy --name preview
alien dev destroy --name preview --token <token>
alien dev vault set my-vault API_KEY secret-value
```

Top-level commands such as `alien release` and `alien deploy` are for non-local managers. They do not silently switch into local mode.

## State and Storage

Local state lives under `.alien/`.

Typical contents:

```text
.alien/
  build/
  command_kv/
  command_storage/
  dev.db
  testing-dev-status.json
```

The exact files depend on which commands you run, but the local manager, build output, and command-server state all live under that directory.

## Environment Variables Injected Into Local Deployments

Local deployments receive the same core Alien runtime variables as other deployments, but with localhost endpoints. That includes the commands polling URL and OTLP log endpoint exposed by the local manager.

User-supplied `--env` and `--secret` values are merged into the deployment environment as part of the local create/update flow.

## What `alien dev` Does Not Do

Current behavior is intentionally simple:

- it does not launch a dashboard
- it does not depend on a TUI
- it does not auto-rebuild on file change
- it does not require any cloud credentials

If you want a new local release after changing code, rerun `alien dev release` or restart `alien dev`.
