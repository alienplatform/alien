import { defineConfig } from "tsdown"

export default defineConfig({
  entry: ["src/index.ts"],
  format: ["esm"],
  dts: {
    sourcemap: true,
  },
  hash: false,
  clean: true,
  sourcemap: true,
})
