# Data Connector

Query databases behind the customer's firewall. Connection credentials stay in the customer's vault -- your code uses them at runtime but never exposes them. Only query results leave the customer's network.

## What's included

| Resource | Type | Description |
|----------|------|-------------|
| `connector` | Function (live) | Runs queries and returns results via commands |
| `credentials` | Vault (frozen) | Database connection strings, stored in the customer's secret manager |
| `cache` | KV (frozen) | Query result cache to avoid repeated round-trips |

### Commands

| Command | Description |
|---------|-------------|
| `test-connection` | Verify database connectivity (never returns the password) |
| `query` | Run a SQL query, optionally with caching |
| `list-tables` | List available tables |

## Local development

```bash
alien dev
```

The template includes sample data so you can test without a real database. In production, replace it with a real database client (`pg`, `mysql2`, etc.) -- the connection string comes from the customer's vault.

In a second terminal:

```bash
# Test the connection
alien dev commands invoke \
  --deployment default \
  --command test-connection \
  --params '{}'

# Run a query
alien dev commands invoke \
  --deployment default \
  --command query \
  --params '{"sql": "SELECT * FROM users"}'

# Same query, with caching
alien dev commands invoke \
  --deployment default \
  --command query \
  --params '{"sql": "SELECT * FROM orders", "useCache": true}'

# List tables
alien dev commands invoke \
  --deployment default \
  --command list-tables \
  --params '{}'
```

## Running tests

```bash
bun test
```

## Learn more

- [Patterns: Remote Worker](https://alien.dev/docs/patterns#remote-worker)
- [Vault reference](https://alien.dev/docs/infrastructure/vault)
- [Remote Commands](https://alien.dev/docs/commands)
