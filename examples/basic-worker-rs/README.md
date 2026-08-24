# Basic Worker (Rust)

The simplest Alien worker, in Rust. An HTTPS endpoint with one command handler, built with Axum.

The worker gets an HTTPS endpoint in its remote environment. Alien creates the infrastructure, while that environment's network controls who can reach it (public internet, employees only, or other services). See [External URLs](https://alien.dev/docs/external-urls).

## What's included

| Resource | Type | Description |
|----------|------|-------------|
| `agent` | Worker (live) | Serverless worker with an HTTPS endpoint and commands |

### Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | Health check |

### Commands

| Command | Description |
|---------|-------------|
| `echo` | Returns whatever you send it |

## Local development

```bash
alien dev
```

In a second terminal:

```bash
# Send a command
alien dev commands invoke \
  --deployment default \
  --command echo \
  --params '{"hello": "world"}'
```

## Running tests

```bash
bun test
```
