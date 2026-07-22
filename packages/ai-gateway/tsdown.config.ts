import { defineConfig } from "tsdown"

export default defineConfig({
  entry: ["src/index.ts", "src/native.ts"],
  // Declarations are emitted by `tsc --emitDeclarationOnly` (see package.json).
  dts: false,
  hash: false,
  ignoreWatch: ".turbo",
  // Never bundle the native addon: the `./alien-ai-gateway.node` specifier in
  // native.ts must survive into the output as a literal so bun can embed it.
  external: [/\.node$/],
})
