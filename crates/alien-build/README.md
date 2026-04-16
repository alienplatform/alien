# alien-build

Build system for Alien applications. Takes a stack with source code, compiles it, assembles OCI images (via `dockdash`), and pushes them to container registries.

Main entry point: `build_stack(stack, settings)` — processes `FunctionCode::Source` into `FunctionCode::Image`, deduplicating builds across resources that share the same source.

## Toolchains

- **TypeScript** — Compiles with Bun, produces a standalone executable
- **Rust** — Cross-compiles with `cargo-zigbuild` for target architecture
- **Docker** — Builds from a user-provided Dockerfile
