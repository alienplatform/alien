# alien-local

Local platform implementation. "Local" is a full Alien platform — applications run as native processes with local implementations of all resource types:

- **Storage** — Filesystem directories
- **KV** — Sled embedded database
- **Vault** — Filesystem-backed secrets
- **Queue** — Local queue implementation
- **Function** — Native processes managed as tokio tasks, with auto-recovery
- **Artifact Registry** — In-process OCI registry server
- **Container** — Docker-based container management

Application code is identical across platforms — the same `storage.put(key, data)` call works on Local, AWS, GCP, and Azure.

Used by `alien dev` for local development, and deployable to any machine via `alien-deploy deploy --platform local`.

## Docker endpoint selection

Local container and sandbox API clients read `DOCKER_CONFIG` (or the home
`.docker` directory) and select an endpoint in this order:

1. A nonempty `DOCKER_HOST`.
2. A nonempty `DOCKER_CONTEXT`.
3. `currentContext` in `config.json`.
4. Docker's platform default socket or named pipe.

The `default` context is synthetic and honors `DOCKER_HOST`. Named contexts use
Docker CLI's `context inspect` output and ignore environment TLS options. Missing or invalid
selected context metadata fails with a structured error; another daemon is never
tried. This matches Docker CLI's source and installed behavior, including host-first
precedence when both environment variables are set.

Unix sockets, Windows named pipes, and plaintext TCP endpoints are supported.
SSH, TLS environment settings, and TCP contexts with TLS material or verification
settings return actionable unsupported-transport errors. Socket contexts ignore
TLS verification settings, as Docker CLI does. TLS is never silently
removed. Image loading through Docker CLI is pinned to the endpoint captured by
the container API client, including after the active context changes.

Docker bridge discovery remains optional for native services on hosts without a
bindable bridge. Configuration and unsupported-transport errors still propagate. Native services
using the default socket do not require Docker CLI; named contexts require the
CLI to inspect their metadata without duplicating its context-store format.

To validate selection against a real engine, create a dedicated empty daemon with
its own Unix socket, data directory, execution directory, and PID file. Use an
isolated `DOCKER_CONFIG`, create and select a context pointing to that socket, and
run the ignored `docker_connection::tests::dedicated_engine_proof` test with
nextest. It must ping the engine, obtain its version, and confirm there are no
containers. `dedicated_engine_baseline` verifies that Bollard's old local-default
connector cannot select that engine. `dedicated_pinned_subprocess` verifies CLI
selection after the persisted context changes. These fixtures perform read-only
API operations and require no application builds or containers.
