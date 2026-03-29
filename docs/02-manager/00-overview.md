# alien-manager

alien-manager is the control plane for Alien applications. It stores releases, deploys them to remote environments, dispatches commands to running deployments, and forwards telemetry. Single binary, SQLite-backed, no external dependencies.

```
┌────────────────────────────────────┐
│          alien-manager              │
│                                    │
│  ┌──────────┐  ┌────────────────┐  │
│  │ REST API │  │ Deployment Loop│  │
│  └────┬─────┘  └───────┬────────┘  │
│       │                │           │
│  ┌────┴─────┐  ┌───────┴────────┐  │
│  │ Commands │  │  Telemetry     │  │
│  └──────────┘  └────────────────┘  │
│       │                            │
│  ┌────┴──────────────────────┐     │
│  │       SQLite (Turso)      │     │
│  └───────────────────────────┘     │
└────────────────────────────────────┘
       ▲                │
       │                ▼
   CLI / SDK       Cloud APIs
 (releases,      (provision resources
  deployments)    in remote environments)
```

## How Deployments Work

This is the core concept. Everything else builds on it.

### Step 1: Developer publishes a release

The developer builds and registers a release with alien-manager:

```sh
alien build --platform aws
alien release
```

This compiles `alien.ts` into OCI images, pushes them to the container registry, and creates a release record. alien-manager now knows what to deploy — but nothing is provisioned yet.

### Step 2: Admin sets up the remote environment

Alien deploys into environments the developer doesn't own — a customer's AWS account, their GCP project, their Kubernetes cluster. Someone with credentials for that environment kicks off the first deployment.

The admin runs:

```sh
alien deploy --name production --platform aws --token ax_dg_...
```

What happens:

1. The CLI asks alien-manager for the deployment's target state
2. It runs `alien-deployment::step()` locally — provisioning infra using the **admin's own cloud credentials**
3. It reports the resulting infra state back to alien-manager

The admin's machine drives the provisioning. alien-manager records the result.

### Step 3: Ongoing updates — push or pull

After initial setup, the two models diverge based on what the admin grants.

**Push mode** — the admin grants alien-manager cross-account credentials (an IAM role, a GCP service account, an Azure service principal). alien-manager's deployment loop impersonates that service account and calls cloud APIs in the remote environment directly whenever a new release is available. No further admin action needed.

Best for: AWS, GCP, Azure.

**Pull mode** — instead of granting credentials, the admin installs an Agent in the remote environment. The Agent polls alien-manager for updates and runs `alien-deployment::step()` locally using its own in-cluster credentials. alien-manager never touches the remote environment directly.

Best for: Kubernetes, edge devices, or anywhere cross-account access isn't an option.

Both modes run the same `alien-deployment::step()` function. The difference is only who runs it and where the credentials come from.

| | Push | Pull |
|---|---|---|
| **Who runs `step()`** | alien-manager's deployment loop | Agent in the remote environment |
| **Credentials** | Configured on alien-manager (env vars) | Local to the remote environment |
| **Initial setup** | Admin runs `alien deploy` or equivalent | Admin installs Agent with a token |
| **Updates** | Automatic — server pushes on new release | Automatic — Agent polls for updates |
| **Cross-account access** | Required | Not required |

## Data Model

**Deployments** — a running instance of your application in a remote environment. Each targets one platform (AWS, GCP, Azure, Kubernetes, or Local) and tracks its provisioning state. See [Deployments](01-deployments.md).

**Releases** — an immutable snapshot of your application's built stack. Contains platform-keyed stack definitions compiled from `alien.ts`. A release can target multiple platforms simultaneously. See [Releases](02-releases.md).

**Deployment groups** — logical grouping of deployments. Controls fleet size (`max_deployments`) and provides scoped tokens for deployment creation. Use one group per environment, or one per customer for fleet deployments.

**Tokens** — Bearer tokens for API authentication, stored as SHA-256 hashes. Three types: admin (full access), deployment group (create deployments within the group), deployment (OTLP ingestion, command polling). See [Authentication](05-auth.md).

**Commands** — remote command execution records. Created by the caller, picked up by deployments via lease polling, executed, and responded to. See [Commands](03-commands.md).

## Provider Architecture

alien-manager uses trait-based providers for its core subsystems. Each has a default implementation and can be replaced.

### Data stores

Three traits handle persistence, each focused on one domain:

**DeploymentStore** — deployment CRUD, status transitions, coordination (acquire/reconcile/release), and deployment group management. Default: `SqliteDeploymentStore`.

**ReleaseStore** — release CRUD (create, get, get_latest). Default: `SqliteReleaseStore`.

**TokenStore** — token creation and validation (SHA-256 lookup). Default: `SqliteTokenStore`.

All three SQLite implementations share the same database file.

### Other providers

**CredentialResolver** — provides credentials for the target remote environment where push-model deployments run. Default: `EnvironmentCredentialResolver` — reads `AWS_*`, `GOOGLE_*`, `AZURE_*` from environment variables.

**TelemetryBackend** — receives OTLP logs, traces, and metrics from deployments. Default: `OtlpForwardingBackend` — forwards to an external observability endpoint. Dev mode: `InMemoryTelemetryBackend` — stores in a ring buffer for the CLI TUI. See [Telemetry](04-telemetry.md).

**AuthValidator** — validates Bearer tokens and resolves the caller's identity. Default: `TokenDbValidator` — looks up hashed tokens via `TokenStore`. See [Authentication](05-auth.md).

**ServerBindings** — resources alien-manager needs for its own operation: Command KV (state, leases), Command Storage (large payloads), CommandDispatcher (push delivery to Lambda/PubSub/Service Bus), CommandRegistry (metadata). Default dispatcher: `DefaultCommandDispatcher` — pushes commands to push-capable deployments. Default KV and storage: local filesystem. Default registry: `SqliteCommandRegistry`.

### Bindings provider

The deployment loop needs access to cloud-specific resources: an `ArtifactRegistry` for cross-account access management, and credentials for push dispatch. These come from a `BindingsProviderApi` implementation.

alien-manager is a standalone Docker image — not an Alien application. It doesn't use alien-runtime or gRPC-based bindings. Instead, it uses a `BindingsProvider` in **Direct mode**, which reads `ALIEN_*_BINDING` environment variables and constructs implementations directly:

```bash
# Configure artifact registry (required for cross-account access)
ALIEN_ARTIFACTS_BINDING='{"type":"artifact-registry","platform":"aws","region":"us-east-1"}'
```

The binding tells the server what type of registry to use (ECR, Artifact Registry, ACR) and how to reach it. Cloud credentials come from the standard `AWS_*`, `GOOGLE_*`, `AZURE_*` environment variables — the same ones `CredentialResolver` uses.

If no artifact registry binding is configured, cross-account access operations are skipped. This is fine for single-account deployments where the compute resources already have pull access to the registry.

For **Command KV and Storage**, the standalone server uses local filesystem defaults. These are constructed directly — no binding configuration needed.

For **push dispatch**, `DefaultCommandDispatcher` resolves credentials via `CredentialResolver` and looks up the push endpoint from each deployment's stack state. No additional bindings needed.

## Embeddable

alien-manager is a library. The builder + traits IS the architecture — there is no "mode" concept in the library itself. Each call site wires the builder with appropriate trait implementations:

### Standalone

The `alien-manager` binary (`bin/main.rs`). Single-tenant, single-project. SQLite stores, environment-based credentials, token-based auth. This is the OSS distribution. Uses the `with_standalone_defaults()` convenience method:

```rust
AlienManager::builder(config)
    .token_store(bootstrapped_token_store)
    .with_standalone_defaults()
    .await?
    .build()
    .await?
```

### Dev

Started by `alien dev`. The CLI embeds the library and wires local-only providers explicitly: `LocalCredentialResolver`, `PermissiveAuthValidator`, `InMemoryTelemetryBackend`, local SQLite stores, and local command bindings. The default `local-dev` deployment group is created by the CLI after startup, not by the manager builder.

```rust
AlienManager::builder(config)
    .deployment_store(dev_deployment_store)
    .release_store(dev_release_store)
    .token_store(dev_token_store)
    .credential_resolver(local_credentials)
    .telemetry_backend(in_memory_telemetry)
    .auth_validator(permissive_auth)
    .server_bindings(local_bindings)
    .build()
    .await?
```

### Platform

Used by managed platforms that embed alien-manager in their own binary. The call site creates all providers explicitly — API-backed stores, multi-tenant auth, platform-specific credential resolution, custom telemetry pipeline — and passes them to the builder:

```rust
AlienManager::builder(config)
    .deployment_store(my_deployment_store)
    .release_store(my_release_store)
    .token_store(my_token_store)
    .credential_resolver(my_credential_resolver)
    .telemetry_backend(my_telemetry_backend)
    .auth_validator(my_auth_validator)
    .server_bindings(my_server_bindings)
    .extra_routes(my_additional_routes)
    .build()
    .await?
```

All providers must be set explicitly (either one-by-one or via `with_standalone_defaults()`). The core logic — API handlers, deployment loop, command server, sync protocol — is shared across all configurations.

## Crate Location

`alien/crates/alien-manager/`

Dependencies: `alien-deployment`, `alien-commands`, `alien-bindings`, `alien-core`, `alien-infra`.
