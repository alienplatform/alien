# GitHub Agent

A full application built on Alien: a GitHub integration that runs inside the customer's cloud, with a Next.js dashboard as the control plane in yours. The worker analyzes and labels pull requests using the customer's own GitHub token -- the token is stored in their vault and never leaves their environment. Only analysis results come back to the dashboard.

```
Your Cloud                          Customer's Cloud
+-----------------+                 +----------------------+
|  Dashboard      |  -- command --> |  worker              |
|  (Next.js)      |  <-- result --  |  (analyze, label)    |
+-----------------+                 |                      |
                                    |  +-- integrations -+ |
                                    |  | Vault: GitHub   | |
                                    |  | tokens          | |
                                    |  +-----------------+ |
                                    +----------------------+
```

## What's included

| Resource | Type | Description |
|----------|------|-------------|
| `agent` | Worker (live) | Commands-enabled worker, also reachable over HTTP |
| `integrations` | Vault (frozen) | GitHub tokens and repo configuration, stored in the customer's secret manager |

The repo has two packages:

- [`packages/remote-agent`](./packages/remote-agent) -- the worker that deploys into the customer's environment
- [`packages/dashboard`](./packages/dashboard) -- the Next.js control plane you host yourself

### Commands

| Command | Description |
|---------|-------------|
| `set-integration` | Store a GitHub owner/repo/token config in the vault |
| `analyze-repository` | Fetch and classify pull requests, compute metrics |
| `label-pull-requests` | Apply labels to pull requests based on classification |

### HTTP endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | Health check |
| GET | `/prs` | List analyzed pull requests |

## Local development

```bash
alien init github-agent
cd github-agent
```

Start the worker:

```bash
cd packages/remote-agent
alien dev
```

Demo mode is built in -- everything works without a real GitHub token. To run the dashboard against it, follow [`packages/dashboard/README.md`](./packages/dashboard/README.md).

### Send a command

In a second terminal:

```bash
# Configure an integration (demo mode, no token needed)
alien dev commands invoke \
  --deployment default \
  --command set-integration \
  --params '{"integrationId": "demo", "config": {"owner": "acme", "repo": "web"}}'

# Analyze the repository
alien dev commands invoke \
  --deployment default \
  --command analyze-repository \
  --params '{"integrationId": "demo"}'
```

## Running tests

```bash
cd packages/remote-agent
bun test
```

## Learn more

- [Patterns: Control Plane / Data Plane](https://alien.dev/docs/patterns)
- [Remote Commands](https://alien.dev/docs/commands)
- [Vault reference](https://alien.dev/docs/infrastructure/vault)
- [alien.dev](https://alien.dev) -- ship to your customer's cloud, keep it fully managed
