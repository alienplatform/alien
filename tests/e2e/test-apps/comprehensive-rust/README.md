# alien-test-server

A test server for integration testing Alien applications.

## Building

Before running tests, you need to build the test server binary:

```bash
# For Linux (x86_64)
cargo build --release --target x86_64-unknown-linux-musl --bin alien-test-server

# For Linux (ARM64/aarch64)
cargo build --release --target aarch64-unknown-linux-musl --bin alien-test-server

# For local development/testing
cargo build --release --bin alien-test-server
```

The `alien.ts` automatically determines the correct binary path based on your platform.

## Configuration

The server configuration is defined in `alien.ts`. It includes:
- A storage resource (`test-alien-storage`)
- A management role with full stack permissions
- A function role with storage read/write permissions
- A public function that runs the test server

## Endpoints

The test server provides the following endpoints:
- `/hello` - Returns a simple greeting
- `/inspect` - Echoes back the request body
- `/env-var/:name` - Gets an environment variable value
- `/storage-test/:binding_name` - Tests storage operations
- `/vault-test/:binding_name` - Tests vault operations (set, get, delete, verify deletion)
- `/sse` - Server-sent events stream

## Running Tests

Use the `alien test` command to deploy and test:

```bash
alien test --aws-account-id YOUR_ACCOUNT_ID
```

See `example.integration.test.ts` for example tests. 