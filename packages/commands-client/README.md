# @aliendotdev/commands-client

Lightweight TypeScript client for Alien command invocation.

## Installation

```bash
npm install @aliendotdev/commands-client
```

## Usage

```typescript
import { CommandsClient } from "@aliendotdev/commands-client"

const commands = new CommandsClient({
  managerUrl: "https://manager.example.com",
  deploymentId: "dp_123",
  token: "bearer_token",
})

const result = await commands.invoke("generate-report", {
  startDate: "2024-01-01",
  endDate: "2024-01-31",
})

console.log(result)
```

## Features

- Automatic base64 encoding/decoding for inline payloads
- Polling with exponential backoff
- Structured error handling via `AlienError`
- Configurable timeout and idempotency options
- Node.js and browser compatible

## API

```typescript
class CommandsClient {
  constructor(config: CommandsClientConfig)
  invoke<TParams, TResponse>(
    command: string,
    params: TParams,
    options?: InvokeOptions,
  ): Promise<TResponse>
}
```

```typescript
interface CommandsClientConfig {
  managerUrl: string
  deploymentId: string
  token: string
  timeout?: number
  allowLocalStorage?: boolean
}
```

## Limitations

- Storage-mode param uploads are not yet supported
- Large responses are supported only when the server provides downloadable storage payloads

## Development

```bash
pnpm build
pnpm test:ts
pnpm format-and-lint
```

## License

ISC
