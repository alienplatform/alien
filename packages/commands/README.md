# `@alienplatform/commands`

Send commands to an Alien deployment or receive them from any Node or Bun
process. For hosted Alien, an ordinary app needs no `alien.ts`, manager URL, or
manually minted token. The package is pure TypeScript over `fetch`.

## Install

```sh
npm install @alienplatform/commands
```

Requires Node 18+ or Bun 1.0.23+.

## Send from an external app

Connect with the deployment ID and a server-side Alien API key:

```ts
import { CommandsClient } from "@alienplatform/commands"

const commands = await CommandsClient.forDeployment({
  deploymentId: "dep_123",
  apiKey: process.env.ALIEN_API_KEY!,
})

const result = await commands
  .target("image-processor")
  .invoke<{ ok: boolean }>("health-check", {})
```

Alien discovers the deployment's current manager and mints short-lived,
commands-only access. The API key is sent only to the Alien API; the client uses
the short-lived credential with the manager and refreshes it internally.

## Receive in an external app

Name the command-enabled Container or Daemon this process serves:

```ts
import { createCommandReceiver } from "@alienplatform/commands"

const receiver = createCommandReceiver({
  deploymentId: "dep_123",
  apiKey: process.env.ALIEN_API_KEY!,
  target: "image-processor",
})

receiver.command("health-check", async () => ({ ok: true }))

process.on("SIGTERM", () => receiver.stop())
await receiver.run()
```

The target is explicit because one deployment can contain several
command-capable resources. Discovery starts when `run()` starts; Alien resolves
whether the target is a Container or Daemon and issues short-lived access
limited to that deployment and target.

Apps deployed by Alien remain zero-config because Alien injects their receiver
identity and credentials:

```ts
const receiver = createCommandReceiver()
receiver.command("health-check", async () => ({ ok: true }))
await receiver.run()
```

## Self-hosted manager

The explicit constructor remains available for self-hosting and local
development:

```ts
const commands = new CommandsClient({
  managerUrl: "http://localhost:8080",
  deploymentId: "dep_123",
  token: process.env.ALIEN_COMMANDS_TOKEN!,
})
```

For an explicit receiver, configure the existing `ALIEN_COMMANDS_*`
environment variables and call `createCommandReceiver()`.

Both hosted APIs accept `platformUrl` when the Alien Platform API is not at
`https://api.alien.dev`.
