# Event Pipeline

React to things happening in the customer's environment -- queue messages, file uploads, and scheduled jobs. All three trigger types in a single function.

## What's included

| Resource | Type | Description |
|----------|------|-------------|
| `processor` | Function (live) | Handles queue messages, storage events, and cron triggers |
| `inbox` | Queue (frozen) | Message queue (SQS / Pub/Sub / Service Bus) |
| `data` | Storage (frozen) | Object storage -- triggers on file creation |
| `events` | KV (frozen) | Logs processed events for querying |

### Triggers

| Type | Source | Description |
|------|--------|-------------|
| Queue | `inbox` | Fires when a message arrives |
| Storage | `data` | Fires when a file is created |
| Cron | `0 * * * *` | Fires every hour |

### Commands

| Command | Description |
|---------|-------------|
| `get-events` | Query processed events, optionally filter by type |
| `get-stats` | Count of processed events by type |
| `send-test-message` | Send a test message to the inbox queue |

## Local development

```bash
alien dev
```

In a second terminal:

```bash
# Send a message to the queue
alien dev commands invoke \
  --deployment default \
  --command send-test-message \
  --params '{"message": "hello from queue"}'

# Wait a few seconds for processing, then check events
alien dev commands invoke \
  --deployment default \
  --command get-events \
  --params '{"type": "queue"}'

# Check stats across all trigger types
alien dev commands invoke \
  --deployment default \
  --command get-stats \
  --params '{}'
```

## Running tests

```bash
bun test
```

## Learn more

- [Events & Triggers](https://alien.dev/docs/infrastructure/function/events-and-triggers)
- [Queue reference](https://alien.dev/docs/infrastructure/queue)
- [Storage reference](https://alien.dev/docs/infrastructure/storage)
