# Build

## Overview

The `alien build` command validates the stack, compiles source code, and produces OCI images (the standard container image format — same as Docker images).

## alien build

The `alien build` command:

1. Loads `alien.config.ts`
2. **Validates** the stack with build-time preflights (see [Preflights](04-preflights.md))
3. Builds each compute resource's source code using the appropriate toolchain
4. Packages everything into OCI images
5. Generates platform-specific templates (CloudFormation for AWS)
6. Outputs everything to `.alien/build/{platform}/`

Build fails fast if validation fails or any compilation fails.

```bash
# Build for a platform (required)
alien build --platform aws
alien build --platform gcp
alien build --platform local

# Build with remote cache
alien build --platform aws --cache-url s3://my-bucket/build-cache

# Build for specific targets
alien build --platform aws --targets linux-x64,linux-arm64
```

### Build Output

After building, `.alien/` contains:

```
.alien/
└── build/
    └── aws/
        ├── stack.json                    # Compiled stack definition
        ├── cloudformation_template.yaml  # Platform template
        └── my-function/
            └── linux-aarch64.oci.tar     # OCI image tarball
```

### Source → Image Transformation

Compute resources (Function, Container, Worker) can specify source code in `alien.config.ts`:

```json
{
  "code": {
    "type": "source",
    "src": "./src",
    "toolchain": { "type": "typescript" }
  }
}
```

The build process compiles this and transforms it into an image reference:

```json
{
  "code": {
    "type": "image",
    "image": "/path/to/.alien/build/aws/my-function"
  }
}
```

At this point, `image` points to a local directory containing OCI tarballs.

> **Note:** Containers can also use pre-built images (`{ type: "image", image: "postgres:16" }`). These skip compilation but are still validated and included in the output stack.

## Toolchains

A **toolchain** compiles source code for a target platform. Alien detects the toolchain from the resource's `.code()` configuration:

```typescript
// Function
new alien.Function("api")
  .code({ type: "source", toolchain: { type: "typescript" }, src: "./api" })

// Or specify custom binary name
new alien.Function("api")
  .code({ type: "source", toolchain: { type: "typescript", binaryName: "my-api" }, src: "./api" })

// Container
new alien.Container("worker")
  .code({ type: "source", toolchain: { type: "rust", binaryName: "worker" }, src: "./worker" })

// Worker
new alien.Worker("agent")
  .code({ type: "source", toolchain: { type: "typescript" }, src: "./agent" })
```

### TypeScript Toolchain

Uses **Bun** for compilation to a single executable. Supports any package manager for dependency installation.

**Configuration:**
```typescript
{ type: "typescript" }                           // Uses package.json name
{ type: "typescript", binaryName: "my-server" }  // Custom binary name
```

**Build Process:**

1. Installs dependencies (works with any lockfile - `bun.lockb`, `pnpm-lock.yaml`, `package-lock.json`)
2. Detects entry point from `package.json` (`main` field) or uses `./src/index.ts` by default
3. Compiles to single executable:
   ```bash
   bun build --compile --target {bun-target} --outfile {binary_name} {entry_point}
   ```
4. Packages the compiled binary

The binary name defaults to the `name` field in `package.json`, or can be specified explicitly in the toolchain config.

Runtime command: `./{binary_name}`

### Rust Toolchain

Uses `cargo-zigbuild` for cross-compilation.

1. Installs target if needed (`rustup target add`)
2. Runs `cargo zigbuild --target {target} --bin {binary_name} --release`
3. Packages the compiled binary

Runtime command: `./{binary_name}`

## Build Targets

Each platform has default build targets:

| Platform   | Default Targets |
|------------|-----------------|
| AWS        | `linux-arm64`   |
| GCP        | `linux-x64`     |
| Azure      | `linux-x64`     |
| Kubernetes | `linux-arm64`   |
| Local      | All targets     |

Override with `--targets`:

```bash
alien build --platform aws --targets linux-x64,linux-arm64
```

Available targets: `linux-x64`, `linux-arm64`, `darwin-arm64`, `windows-x64`.

## OCI Image Structure

Each built image contains:

```
/app/
├── alien-runtime       # Alien runtime binary
└── {binary_name}       # Application binary (Rust or compiled TypeScript)
```

Both Rust and TypeScript produce a single executable. TypeScript uses `bun build --compile` to bundle the application and all dependencies into one binary.

The Dockerfile structure:

```dockerfile
ENTRYPOINT ["alien-runtime", "--"]
CMD ["./{binary_name}"]
```

See `04-runtime/00-runtime.md` for details on how the runtime starts and manages the application.

## Build Caching

Toolchains support remote caching to speed up builds:

```bash
alien build --platform aws --cache-url s3://my-bucket/build-cache
```

The cache stores:
- **Rust**: `~/.cargo/registry`, `~/.cargo/git`, `target/`
- **TypeScript**: `~/.bun/install/cache/`, `node_modules/`

Cache keys are derived from lock files (`Cargo.lock`, `bun.lockb`, etc.) and build target.

## Implementation

The `alien-build` crate provides two main functions:

- `build_stack(stack, settings)` - Validates and compiles source to OCI images
- `push_stack(stack, platform, push_settings)` - Pushes images to registry

Both build compute resources in parallel with fail-fast behavior.

The toolchain system is defined in `alien-build/src/toolchain/`:

- `mod.rs` - `Toolchain` trait and `create_toolchain()` factory
- `rust.rs` - Rust/Cargo toolchain
- `typescript.rs` - TypeScript toolchain with package manager detection
- `cache_utils.rs` - Remote caching utilities


