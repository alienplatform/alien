# alien-worker-runtime

In-container Worker runtime — starts user code and translates platform invocations into the Worker app protocol.

## Startup Sequence

1. Starts the Worker app protocol server (Control + WaitUntil)
2. Loads Worker secrets from vault, keeping runtime-only secrets out of user code
3. Starts the application as a subprocess
4. Waits for the app to register its HTTP port
5. Enables authenticated command push when configured
6. Starts the platform-appropriate transport

## Transports

Platform-specific request routing:
- **Lambda** — AWS Lambda event handler
- **Cloud Run** — HTTP transport for GCP
- **Container Apps** — Azure HTTP transport
- **Local/HTTP** — HTTP forwarding plus authenticated Worker command push

## Bindings

Applications use `alien-bindings` directly in-process. Binding operations are
not part of the Worker app protocol.

`BindingsSource` controls how bindings are obtained:
- `FromEnvironment` — Production: create providers from env vars
- `Provider` — Dev/test: use a pre-built provider
