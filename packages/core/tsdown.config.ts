import { defineConfig } from "tsdown"

export default defineConfig({
  entry: ["src/index.ts", "src/tests/index.ts"],
  dts: {
    sourcemap: true,
  },
  hash: false,
  ignoreWatch: ".turbo",
})
