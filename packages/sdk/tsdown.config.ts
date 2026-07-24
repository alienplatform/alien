import { defineConfig } from "tsdown"

export default defineConfig({
  entry: ["src/index.ts", "src/worker-runtime/index.ts", "src/native.ts"],
  // Use tsc for declaration files instead of tsdown's built-in dts.
  // With two entry points that share types, tsdown's rolldown-based dts
  // generation code-splits the .d.ts output, putting shared types into
  // index.d.ts (as a chunk with minified names) and the actual main entry
  // types into index2.d.ts. This breaks the package.json "types" field.
  dts: false,
  hash: false,
  ignoreWatch: ".turbo",
  // Bundle all dependencies so the package is self-contained.
  // This allows bun build --compile to work without needing
  // transitive dependencies installed in the user's project.
  //
  // Exception: @alienplatform/bindings and @alienplatform/ai-gateway must stay
  // external. Each holds a native addon loader's process-level state (the
  // embedded-addon registration used by compiled binaries). Inlining a copy here
  // would give the SDK its own loader instance, so a compiled worker's
  // `installEmbeddedAddon()` (which registers into the real modules via their
  // `/native` subpaths) would not be seen by the SDK's re-exported
  // `kv`/`storage`/`queue`/`vault` (bindings) or `ai`/`getAiConnection`
  // (ai-gateway). Keeping them external makes the compiled binary share one
  // module for each. They're real runtime dependencies, so the package stays
  // self-contained.
  external: [
    "@alienplatform/bindings",
    "@alienplatform/bindings/native",
    "@alienplatform/ai-gateway",
    "@alienplatform/ai-gateway/native",
  ],
  noExternal: [/.*/],
})
