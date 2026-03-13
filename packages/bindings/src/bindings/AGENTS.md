# Binding Implementations

Each TypeScript binding must match its Rust counterpart in `alien-bindings/src/providers/<name>/grpc.rs`.

**Examples:**
- `kv.ts` → `providers/kv/grpc.rs`
- `storage.ts` → `providers/storage/grpc.rs`
- `build.ts` → `providers/build/grpc.rs`

## Core Pattern: Public API → gRPC → Public API

```
┌─────────────────┐       ┌─────────────────┐       ┌─────────────────┐
│   Public API    │  →    │   gRPC Proto    │  →    │   Public API    │
│   (SDK types)   │       │   (wire format) │       │   (SDK types)   │
└─────────────────┘       └─────────────────┘       └─────────────────┘
     User calls              Internal only            User receives
```

**Public API types** come from `@aliendotdev/core` or `../types.ts`. Proto types are **internal only** - never exposed to users.

```typescript
// ❌ Bad - exposing proto types
async getStatus(buildId: string): Promise<BuildExecutionProto>

// ✅ Good - convert proto to public types
async getStatus(buildId: string): Promise<BuildExecution> {
  const response = await this.client.getBuildStatus({ ... })
  return this.fromProtoExecution(response.execution!)  // Transform here
}
```

## Guidelines

### 1. Match All Rust Trait Methods
Every method in the Rust trait must have a TypeScript equivalent. Check for missing methods when adding bindings.

### 2. Match Rust Enum Fallbacks Exactly
Rust often maps unknown/unspecified enum values to a specific fallback. TypeScript must match:

```rust
// Rust: maps Unspecified → Failed
BuildStatus::Unspecified => BuildStatus::Failed,
```
```typescript
// TypeScript: must match
[BuildStatusProto.BUILD_STATUS_UNSPECIFIED]: "FAILED",  // NOT "QUEUED"
```

### 3. Transform Types in Private Helpers
Keep transformations consistent and testable:

```typescript
private fromProtoExecution(proto: BuildExecutionProto): BuildExecution {
  return {
    id: proto.id,
    status: statusMap[proto.status] ?? "FAILED",
    startTime: proto.startTime ? new Date(proto.startTime) : undefined,
  }
}

private toProtoConfig(config: BuildStartConfig): BuildConfigProto {
  return {
    script: config.script,
    computeType: computeTypeMap[config.computeType] ?? ComputeType.UNSPECIFIED,
  }
}
```

### 4. Convert Units Consistently
Rust uses `Duration`, TypeScript uses milliseconds:

```typescript
ttlSeconds: options.ttlMs ? Math.floor(options.ttlMs / 1000) : undefined
timeoutSeconds: request.timeoutMs ? Math.floor(request.timeoutMs / 1000) : undefined
```

### 5. Use wrapGrpcCall for Error Handling
Always wrap gRPC calls with context for proper error mapping:

```typescript
return await wrapGrpcCall(
  "BuildService",
  "GetBuildStatus",
  async () => { ... },
  { bindingName: this.bindingName },  // Context for error mapping
)
```

### 6. Handle Optional Proto Fields
Rust `Option<T>` becomes `T | undefined`. Use nullish coalescing:

```typescript
uri: proto.uri ?? "",
createdAt: proto.createdAt ? new Date(proto.createdAt) : undefined,
```

## Convenience Methods

TypeScript may add convenience methods not in Rust (e.g., `getJson()`, `exists()`, `waitForCompletion()`). These are fine if they build on core methods.

## See Also

- Parent: `packages/bindings/AGENTS.md` - Package overview and type architecture
- Proto definitions: `crates/alien-bindings/proto/`
