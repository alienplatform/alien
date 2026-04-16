# alien-cli

Developer-facing CLI for Alien.

## Commands

- **`alien init`** — Scaffold a new project from a template
- **`alien build`** — Build the application into OCI images
- **`alien release`** — Push images and create a release on the manager
- **`alien onboard`** — Create a deployment group and generate a deployment link
- **`alien deployments`** — List and manage deployments
- **`alien deploy`** / **`alien destroy`** — Deploy to or destroy from a cloud platform
- **`alien vault`** — Manage vault secrets for a deployment
- **`alien commands`** — Invoke remote commands on deployments
- **`alien serve`** — Start a standalone alien-manager server
- **`alien dev`** — Local development (embeds manager + runtime, hot reload)
  - `alien dev server` — Start only the local manager
  - `alien dev deploy` / `alien dev destroy` — Deploy/destroy against local manager
  - `alien dev release` / `alien dev vault` / `alien dev commands` — Local variants

## Features

- `otlp` (default) — OpenTelemetry forwarding
- `platform` — OAuth/keyring for platform authentication, adds `login`, `workspaces`, `projects`, `link`, `manager` commands
