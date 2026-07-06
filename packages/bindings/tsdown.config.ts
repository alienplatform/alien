import { defineConfig } from "tsdown"

export default defineConfig({
  entry: ["src/index.ts", "src/native.ts"],
  // Declarations are emitted by `tsc --emitDeclarationOnly` (see package.json).
  // tsdown's rolldown-based dts code-splits shared types across the two entries,
  // which breaks the package.json "types" fields — same reason as @alienplatform/sdk.
  dts: false,
  hash: false,
  ignoreWatch: ".turbo",
  // Bundle all dependencies so `./native` is self-contained for bun --compile.
  noExternal: [/.*/],
  // ...but never bundle the native addon: the `./alien-bindings.node` specifier
  // in native.ts must survive into the output as a literal so bun can embed it.
  external: [/\.node$/],
})
