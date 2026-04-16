# alien-runtime

In-container runtime — starts user code with injected bindings and routes requests via platform-specific transports.

## Startup Sequence

1. Starts a gRPC server (bindings + control service)
2. Loads secrets from vault (including commands token)
3. Starts the application as a subprocess
4. Waits for the app to register its HTTP port
5. Starts commands polling (if enabled)
6. Starts the platform-appropriate transport

## Transports

Platform-specific request routing:
- **Lambda** — AWS Lambda event handler
- **Cloud Run** — HTTP transport for GCP
- **Container Apps** — Azure HTTP transport
- **Local** — Local development transport
- **Commands polling** — Pull-based command polling from manager

## Bindings Integration

Creates a `BindingsProvider` (from `alien-bindings`) and exposes it via gRPC. Applications call the gRPC server to access storage, KV, vault, and other bindings.

`BindingsSource` controls how bindings are obtained:
- `FromEnvironment` — Production: create providers from env vars
- `Provided` — Dev/test: use pre-built `BindingsProvider`
