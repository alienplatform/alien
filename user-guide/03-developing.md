# Developing

```bash
alien dev
```

Builds your stack and deploys it locally with Docker. Hot-reloads on code changes.

`alien dev` is local-only — it starts a local alien-manager in dev mode with permissive auth, no cloud credentials needed. For cloud deployment, use `alien deploy` with a standalone manager or the platform.

## Dev Server

`alien dev` starts an interactive TUI showing deployment status, logs, and rebuild triggers.

To start just the server without the TUI:

```bash
alien dev server
```

## Dev Subcommands

```bash
alien dev deployments            # List dev deployments
alien dev release                # Push a new release
alien dev destroy                # Tear down dev deployment
```

## Next

- [Onboarding Customers](04-onboarding.md) — deploy to your customers' environments
