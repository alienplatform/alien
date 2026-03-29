# Standalone Mode

Alien is fully open-source. You can run the entire system without an alien.dev account.

```bash
alien-manager
```

This starts a self-contained manager backed by SQLite. No external dependencies, no network calls to alien.dev.

## When to Use Standalone

- **Evaluation** — try Alien without creating an account
- **Air-gapped environments** — no outbound internet access
- **Full independence** — zero dependency on any external service
- **Contributing** — develop and test Alien itself

For production use with cloud platforms, the [alien-hosted tier](https://alien.dev) or [private manager](08-private-manager.md) are recommended — they provide managed TLS, container orchestration, and the dashboard.

## Setup

```bash
alien-manager
```

On first run, the manager generates an admin token:

```
Generated admin token (save this securely):
  ax_admin_abc123def456...

  export ALIEN_MANAGER_URL=http://localhost:8080
  export ALIEN_API_KEY=ax_admin_abc123def456...
```

The manager stores everything in SQLite at `alien-manager.db`.

## Usage

With the admin token set, use the CLI as normal:

```bash
export ALIEN_MANAGER_URL=http://localhost:8080
export ALIEN_API_KEY=ax_admin_abc123def456...

alien release --platform local
alien onboard my-fleet
```

The `ALIEN_MANAGER_URL` variable tells the CLI to talk to your standalone manager instead of alien.dev.

## Feature Comparison

| Feature | Standalone | alien.dev |
|---------|-----------|-----------|
| Local platform (Docker) | Full | Full |
| Kubernetes platform | Full | Full |
| Cloud functions (private ingress) | Full | Full |
| Cloud functions (public HTTPS) | — | `*.vpc.direct` + managed TLS |
| Containers on cloud VMs | — | Horizon orchestration |
| Custom domains | — | Managed DNS + TLS |
| White-labeled CLI / packages | — | Auto-generated |
| Dashboard | — | Web UI |
| Telemetry | OTLP forwarding | DeepStore + dashboard |

Public HTTPS endpoints on cloud platforms require managed TLS certificates and DNS — infrastructure that's genuinely hard to self-host. Containers on cloud VMs require Horizon, Alien's container orchestration system. These aren't artificial limitations.

If your stack uses these features in standalone mode, the build will tell you:

```
Preflight check failed:

  This stack requires an alien.dev account:
    - Public ingress functions require managed TLS (*.vpc.direct domains).

  Sign up at https://alien.dev and run `alien login`.
  Or use --platform local / --platform kubernetes.
```

## Configuration

| Environment Variable | Description | Default |
|---------------------|-------------|---------|
| `PORT` | Server port | `8080` |
| `ALIEN_DB_PATH` | SQLite database path | `alien-manager.db` |
| `ALIEN_STATE_DIR` | State directory | `.alien-manager` |
| `BASE_URL` | Public URL (for deploy page, install script) | `http://localhost:{port}` |
| `OTLP_ENDPOINT` | Forward telemetry to this OTLP endpoint | (disabled) |
| `ALIEN_RELEASES_URL` | Base URL for binary downloads | `https://releases.alien.dev` |
| `ALIEN_AGENT_BINARY` | Path to local agent binary (skips download) | (auto-detect) |

## Cloud Credentials

For push-model deployments, the manager needs cloud credentials in the environment:

- **AWS**: `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_REGION`
- **GCP**: `GOOGLE_APPLICATION_CREDENTIALS` or workload identity
- **Azure**: `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_TENANT_ID`

## Building from Source

```bash
git clone https://github.com/alienplatform/alien.git && cd alien

cargo build -p alien-cli -p alien-deploy-cli -p alien-agent
cd examples && pnpm install && cd ..

# Start standalone manager
./target/debug/alien-manager

# In another terminal
export ALIEN_MANAGER_URL=http://localhost:8080
export ALIEN_API_KEY=<token from above>

cd examples/minimal-cloud-agent
../../target/debug/alien build --platform local --no-tui
../../target/debug/alien release --platform local --yes --no-tui
../../target/debug/alien onboard my-fleet

# In a third terminal
./target/debug/alien-deploy up \
  --token <dg_token> \
  --platform local \
  --manager-url http://localhost:8080 \
  --foreground
```
