# `@alienplatform/ai-gateway` — package layout contract

> Contract document. The names, subpaths, and dependency rules below are binding.
> Implementers may not rename anything pinned here.

## Purpose

`@alienplatform/ai-gateway` is the public TypeScript wrapper for the Alien AI
gateway: a thin loader over a napi-rs native addon. The Rust crate `alien-gateway`
is the single gateway implementation (loopback HTTP proxy, per-cloud ambient
credential injection, curated-catalog model rewrite, protocol selection, streaming
pass-through). The TypeScript layer only loads the addon and starts it once per
process, returning the loopback base URL; every request and SSE stream then flows
over the loopback HTTP socket into the Rust gateway. No gateway logic lives in JS,
and nothing crosses the napi boundary per request.

## Public surface — all exports from `"."`

| Export | Kind | Signature sketch | Notes |
|---|---|---|---|
| `startAiGateway` | function | `startAiGateway(): Promise<{ url: string }>` | Starts the in-process gateway once (idempotent) and returns its running handle. Hold it for the process lifetime. |
| `ai` | function | `ai(name: string): Ai` | An OpenAI-compatible client for the named binding (ambient or BYO-key `External`). |
| `getAiConnection` | function | `getAiConnection(name: string): Promise<AiConnection>` | Starts the gateway on first use; returns `{ baseURL, apiKey? }` for any OpenAI-compatible client. |
| `Ai` | class | `chat.completions.create()`, `responses.create()`, `getAvailableModels()` | What `ai(name)` returns. |
| `AiConnection` | type | `{ baseURL: string; apiKey?: string }` | Result of `getAiConnection`. |
| `AiModel`, `ChatCompletionCreateParams`, `ResponseCreateParams` | types | — | Request/response shapes for the `Ai` client. |
| `aiBindingEnvVarName`, `isExternalAiBinding`, `parseAiBinding` | functions | — | Binding-env helpers. |
| `AiBinding`, `AmbientAiBinding`, `ExternalAiBinding` | types | — | Parsed binding shapes. |
| `AiTransportError`, `AiUpstreamError`, `BindingNotFoundError`, `InvalidBindingConfigError` | error classes | — | Typed errors thrown by the `Ai` client. |

### Intentionally not exposed

- No per-cloud credential surface — the Rust gateway injects the ambient credential.
- No gateway logic in JS — the addon loads and starts the gateway; every request and SSE
  stream flows over the loopback HTTP socket into the Rust gateway.

## Exports map

Two entry points; every condition carries `types`.

```jsonc
{
  ".": { "types": "./dist/index.d.ts", "import": "./dist/index.js" },
  "./native": { "types": "./dist/native.d.ts", "import": "./dist/native.js" }
}
```

- `.` — the lazy-loading entry (resolves the per-platform addon on first use).
- `./native` — the static-embed entry for `bun build --compile` (imports the addon
  through the literal `./alien-ai-gateway.node`, staged next to `dist/native.js`).

## Native addon + prebuilds

- Rust addon crate: `crates/alien-ai-gateway-node` (napi `binaryName`
  `alien-ai-gateway-node`, `packageName` `@alienplatform/ai-gateway`).
- The addon loader (`src/loader.ts`) resolves, in order: `ALIEN_AI_GATEWAY_ADDON_PATH`
  → the per-platform prebuild `@alienplatform/ai-gateway-<triple>` (from
  `optionalDependencies`, injected at publish time) → the version-gated locally-built
  addon under `crates/alien-ai-gateway-node`.
- `TRIPLES` (the prebuild set) must mirror `napi.targets` in the crate's
  `package.json`: `darwin-arm64`, `darwin-x64`, `linux-x64-gnu`, `linux-arm64-gnu`.
  Only glibc Linux prebuilds are published; musl throws a clear unsupported-platform
  error.

## Dependency boundaries

- Runtime dependency: `@alienplatform/core` only.
- No JS cloud SDKs, no gRPC, no `@alienplatform/sdk`.
- The source manifest carries no `optionalDependencies`; the release pipeline injects
  them at publish time from `scripts/inject-optional-deps.mjs`.

## Behavior contract

- Importing the package performs no I/O (`sideEffects: false`); the addon loads on the
  first `startAiGateway()` / `getAiConnection()` call.
- The gateway starts once per process and is shared across binding names (routed by
  `/<name>`); the handle is held for the process lifetime.
