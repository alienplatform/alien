# gRPC Services for Alien Bindings

The `alien-bindings/src/grpc` directory implements gRPC services that enable cross-language communication between Alien applications and the Alien runtime. This allows applications written in any language or framework to interact with Alien resources through gRPC calls.

## Architecture

- **gRPC Server**: Runs in the `alien-runtime` process
- **gRPC Client**: Used by applications (any language) to communicate with the runtime
- **Resource Services**: Each Alien resource (Storage, Build, Vault, etc.) has its own gRPC service

## Adding a New Resource gRPC Service

When adding a new resource to Alien, you must implement both:

1. **gRPC Service** (`[resource]_service.rs`):
   - Implements the gRPC server for the resource
   - Follows the pattern of `storage_service.rs`, `build_service.rs`
   - Uses the generated protobuf types from `proto/[resource].proto`

2. **gRPC Provider** (`providers/[resource]/grpc.rs`):
   - Implements the resource trait using gRPC client calls
   - Allows apps to use the resource through gRPC communication
   - Follows the pattern of existing providers like `providers/storage/grpc.rs`

## Key Components

- **Proto definitions**: Located in `proto/` directory
- **Generated code**: Auto-generated from proto files during build
- **Service implementations**: Handle incoming gRPC requests from apps
- **Provider implementations**: Make gRPC calls to the runtime server
- **Server registration**: Add new services to `server.rs`

## Examples

- **Simple resource**: Look at `build_service.rs` and `providers/build/grpc.rs`
- **Complex streaming**: Look at `storage_service.rs` for multipart upload handling
- **Provider pattern**: Check `providers/storage/grpc.rs` for client implementation

Each resource follows the same pattern: define proto → implement service → implement provider → register in server.
