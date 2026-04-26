import { defineConfig } from "vitest/config"

export default defineConfig({
  test: {
    globals: true,
    testTimeout: 300_000, // 5 minutes
  },
})
