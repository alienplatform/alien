import { defineConfig } from "tsdown"

export default defineConfig({
  entry: ["src/index.ts", "src/native.ts"],
  // Declarations are emitted by `tsc --emitDeclarationOnly` (see package.json).
  // tsdown's rolldown-based dts code-splits shared types across the two entries,
  // which breaks the package.json "types" fields — same reason as @alienplatform/sdk.
  dts: false,
  hash: false,
  ignoreWatch: ".turbo",
  // `@alienplatform/core` is a declared runtime dependency and MUST stay
  // external: bundling a copy would give `dist` its own `AlienError` class, so
  // `err instanceof AlienError` would fail across the package boundary. Only
  // `zod` is bundled — it is a devDependency used solely to build the error
  // definitions, so it must not leak into the runtime dependency set.
  noExternal: [/^zod(\/|$)/],
  // Never bundle the native addon: the `./alien-bindings.node` specifier in
  // native.ts must survive into the output as a literal so bun can embed it.
  external: [/\.node$/],
})
