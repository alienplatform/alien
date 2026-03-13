import { defineConfig } from "tsdown"

export default defineConfig({
  entry: ["src/index.ts"],
  format: ["esm"],
  noExternal: [/.*/], // Bundle all dependencies (except node: built-ins)
  dts: {
    sourcemap: true,
  },
  ignoreWatch: ".turbo",
})
