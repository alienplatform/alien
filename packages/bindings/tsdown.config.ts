import { defineConfig } from "tsdown"

export default defineConfig({
  entry: ["src/index.ts"],
  dts: {
    sourcemap: true,
  },
  hash: false,
  ignoreWatch: ".turbo",
  // Bundle all dependencies so the package is self-contained.
  // This allows bun build --compile to work without needing
  // transitive dependencies installed in the user's project.
  noExternal: [/.*/],
})

