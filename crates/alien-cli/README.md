# alien-cli

Developer-facing CLI for Alien.

## Commands

- **`alien init`** — Scaffold a new project from a template
- **`alien build`** — Build the application into OCI images
- **`alien release`** — Push images and create a release on the manager
- **`alien onboard`** — Create a deployment group and generate a deployment link
- **`alien projects packages get`** / **`update --file`** — Read or update installation package settings
- **`alien projects create`** — Create a name-only Platform project for Dashboard, CLI, or agent onboarding
- **`alien deployments`** — List and manage deployments
- **`alien deploy`** / **`alien destroy`** — Deploy to or destroy from a cloud platform
- **`alien vault`** — Manage vault secrets for a deployment
- **`alien commands`** — Invoke remote commands on deployments
- **`alien serve`** — Start a standalone alien-manager server
- **`alien dev`** — Local development (embeds manager + runtime, hot reload)
  - `alien dev server` — Start only the local manager
  - `alien dev deploy` / `alien dev destroy` — Deploy/destroy against local manager
  - `alien dev release` / `alien dev vault` / `alien dev commands` — Local variants

### Installation package settings

Read all settings, including advanced branding, logs, enrollment, and storage defaults:

```sh
alien projects packages get my-project --workspace=my-workspace --json
```

Apply only the settings you want to change:

```json
{
  "helm": {
    "logCollector": { "enabled": true, "mode": "podApi" },
    "runtimePersistence": {
      "enabled": true,
      "storageClassName": "encrypted-gp3",
      "size": "2Gi"
    }
  }
}
```

Save that object as `settings.json`, then run:

```sh
alien projects packages update my-project --workspace=my-workspace --file settings.json
```

Use `--file -` for standard input and `--json` to return the saved configuration.
Omitted fields are preserved, including nested settings. Arrays are replaced.
`{ "helm": { "enabled": false } }` disables generation and keeps its settings;
`{ "helm": null }` removes the package configuration.
Helm requires the Operator image package to be enabled. Enable both in the same
patch if necessary. Settings apply to new installation packages; they do not
modify charts already installed in customer clusters.

Persistence uses a StorageClass the installation cluster already provides, or
`runtimePersistence.existingClaim` for an existing PVC in the install namespace.
An empty storage class uses the cluster default; clusters without one need an
explicit class. These settings do not install a storage driver.

### Local secrets

Avoid passing secret values through `--secret`, because command arguments can be visible to other local processes and shell-history tools. `alien dev` can read a secret from a file or an inherited environment variable instead:

```sh
alien dev --secret-file API_TOKEN="$HOME/.config/example/api-token"
alien dev --secret-env API_TOKEN=SOURCE_API_TOKEN
```

Append target resource IDs after a colon to scope a secret to specific workloads:

```sh
alien dev --secret-file API_TOKEN="$HOME/.config/example/api-token:api,worker"
```

`--secret-file` removes one trailing line ending (`\n` or `\r\n`), which supports files created by common secret managers. The CLI reports the destination key and source on errors, but does not render the loaded value.

## Features

- `otlp` (default) — OpenTelemetry forwarding
- `platform` — OAuth/keyring for platform authentication, adds `login`, `workspaces`, `projects`, `link`, `manager` commands
