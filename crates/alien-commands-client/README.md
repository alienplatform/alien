# alien-commands-client

Rust client for invoking remote commands on Alien deployments. Mirrors the TypeScript `@alienplatform/sdk/commands` package.

Provides `CommandsClient` with `invoke(command_name, params)` — handles command creation, polling for results, and payload decoding (inline or storage-backed).
