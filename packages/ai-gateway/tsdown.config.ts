import { defineConfig } from "tsdown"

export default defineConfig({
  entry: ["src/index.ts", "src/native.ts"],
  // Declarations are emitted by `tsc --emitDeclarationOnly` (see package.json).
  dts: false,
  hash: false,
  ignoreWatch: ".turbo",
  // Never bundle the embedded binary: the `./alien-ai-gateway.bin` specifier in
  // native.ts must survive into the output as a literal so bun can embed it.
  external: [/\.bin$/],
})
