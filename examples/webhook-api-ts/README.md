# Webhook API

Receive webhooks from external services and expose an HTTP API inside the customer's cloud. Incoming events are stored in a KV store and queryable via both HTTP and remote commands.

The function gets an HTTPS endpoint in the customer's environment. Alien creates the infrastructure -- the customer's network controls who can reach it:

- **Public internet** -- for receiving webhooks from SaaS services (GitHub, Stripe, Slack)
- **Employees only** -- behind a VPN or private DNS, for dashboards and internal tools
- **Other services only** -- as an internal API that other services in their environment call

See [External URLs](https://alien.dev/docs/external-urls).

## What's included

| Resource | Type | Description |
|----------|------|-------------|
| `api` | Function (live) | HTTP API with webhook receivers and commands |
| `events` | KV (frozen) | Stores received webhook events |

### HTTP endpoints

| Method | Path | Description |
|--------|------|-------------|
| POST | `/webhooks/:source` | Receive a webhook from any source (e.g. `/webhooks/github`) |
| GET | `/webhooks/:source/recent` | List recent events from a source |
| GET | `/health` | Health check |

### Commands

| Command | Description |
|---------|-------------|
| `get-events` | Query stored events, optionally filter by source |
| `get-stats` | Count of events for given sources |

## Local development

```bash
alien dev
```

In a second terminal:

```bash
# Send a test webhook
curl -X POST http://localhost:<port>/webhooks/github \
  -H "Content-Type: application/json" \
  -d '{"action": "opened", "number": 42}'

# List recent events
curl http://localhost:<port>/webhooks/github/recent

# Query via commands
alien dev commands invoke \
  --deployment default \
  --command get-events \
  --params '{"source": "github"}'

# Get stats
alien dev commands invoke \
  --deployment default \
  --command get-stats \
  --params '{"sources": ["github", "stripe"]}'
```

The port is shown in the `alien dev` output.

## Running tests

```bash
bun test
```

## Learn more

- [External URLs](https://alien.dev/docs/external-urls)
- [KV reference](https://alien.dev/docs/infrastructure/kv)
- [Remote Commands](https://alien.dev/docs/commands)
