# Basic Worker (TypeScript)

The simplest Alien worker, in TypeScript. An HTTPS endpoint with one command handler -- a good starting point for building something from scratch.

The worker gets an HTTPS endpoint in the customer's environment. Alien creates the infrastructure -- the customer's network controls who can reach it (public internet, employees only, or other services). See [External URLs](https://alien.dev/docs/external-urls).

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
| `echo` | Returns whatever you send it, plus a timestamp |

## Local development

```bash
alien init basic-worker-ts
cd basic-worker-ts
alien dev
```

Everything runs locally -- no cloud credentials needed.

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

## Learn more

- [Quickstart guide](https://alien.dev/docs/quickstart)
- [Remote Commands](https://alien.dev/docs/commands)
- [External URLs](https://alien.dev/docs/external-urls)
- [alien.dev](https://alien.dev) -- ship to your customer's cloud, keep it fully managed
