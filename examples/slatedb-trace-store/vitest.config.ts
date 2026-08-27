import { defineConfig } from "vitest/config"

export default defineConfig({
  test: {
    testTimeout: 60_000,
    hookTimeout: 600_000,
    sequence: { concurrent: false },
  },
})
