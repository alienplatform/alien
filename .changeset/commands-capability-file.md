---
"@alienplatform/commands": patch
---

The pull receiver reads the short-lived capability file that an enrolled
Operator mounts (`ALIEN_IDENTITY_PROVIDER=capabilityFile` with
`ALIEN_COMMANDS_CAPABILITY_FILE` and `ALIEN_MANAGER_URL`). It reads the file on
every request, refuses a capability the manager rejected, and fetches fresh
response authority before submitting a result. Receivers that use
`ALIEN_COMMANDS_TOKEN` or `ALIEN_COMMANDS_TOKEN_FILE` are unchanged.
