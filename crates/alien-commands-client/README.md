# alien-commands-client

Rust client for invoking remote commands on Alien deployments. Mirrors the TypeScript `@alienplatform/commands` package.

Provides `CommandsClient` with `invoke(command_name, params)` and
`target(resource_id)` for explicit Worker, Container, or Daemon routing. The
client handles command creation, result polling, and inline or storage-backed
payload decoding.
