# alien-local

Local platform implementation. "Local" is a full Alien platform — applications run as native processes with local implementations of all resource types:

- **Storage** — Filesystem directories
- **KV** — Sled embedded database
- **Vault** — Filesystem-backed secrets
- **Queue** — Local queue implementation
- **Function** — Native processes managed as tokio tasks, with auto-recovery
- **Artifact Registry** — In-process OCI registry server
- **Container** — Docker-based container management

Application code is identical across platforms — the same `storage.put(key, data)` call works on Local, AWS, GCP, and Azure.

Used by `alien dev` for local development, and deployable to any machine via `alien-deploy up --platform local`.
