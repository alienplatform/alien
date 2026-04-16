# Remote Worker

Execute tool calls in your customer's cloud. Your control plane sends commands, the worker runs them inside the customer's environment -- reading files, writing results -- without any data leaving their network.

```
Your Cloud                          Customer's Cloud
+-----------------+                 +----------------------+
|  AI Agent       |  -- command --> |  worker              |
|  (reasoning)    |  <-- result --  |  (this template)     |
+-----------------+                 |                      |
                                    |  +-- files --------+ |
                                    |  | Private storage | |
                                    |  +-----------------+ |
                                    +----------------------+
```

## What's included

| Resource | Type | Description |
|----------|------|-------------|
| `worker` | Function (live) | Serverless function with command handlers |
| `files` | Storage (frozen) | Private file storage per customer (S3 / Cloud Storage / Blob Storage) |

### Commands

| Command | Description |
|---------|-------------|
| `execute-tool` | Run a tool by name (`read-file`, `write-file`) |
| `list-tools` | List all available tools and their descriptions |

## Local development

```bash
alien dev
```

Everything runs locally -- storage on the filesystem, no cloud credentials needed.

### Send a command

In a second terminal:

```bash
# List available tools
alien dev commands invoke --deployment default --command list-tools

# Write a file
alien dev commands invoke \
  --deployment default \
  --command execute-tool \
  --params '{"tool": "write-file", "params": {"path": "hello.txt", "content": "Hello!"}}'

# Read it back
alien dev commands invoke \
  --deployment default \
  --command execute-tool \
  --params '{"tool": "read-file", "params": {"path": "hello.txt"}}'
```

### Simulate multiple customers

```bash
alien dev deploy --name acme-corp --platform local
```

Each deployment gets isolated storage -- files written by `default` are invisible to `acme-corp`.

### Push an update

Change your code, then:

```bash
alien dev release
```

## Running tests

```bash
bun test
```

## Deploy to a real cloud

See [From Local to AWS](https://alien.dev/docs/quickstart/from-local-to-aws) to deploy this worker into a customer's AWS account.

## Learn more

- [Patterns: Remote Worker](https://alien.dev/docs/patterns#remote-worker)
- [Remote Commands](https://alien.dev/docs/commands)
- [Stacks](https://alien.dev/docs/stacks)
