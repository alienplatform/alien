import { defineConfig } from "vitest/config"

export default defineConfig({
  test: {
    include: ["tests/**/*.test.ts"],
    testTimeout: 300_000, // 5 min timeout for tests involving deployment
    hookTimeout: 300_000, // 5 min for beforeAll/afterAll
    pool: "forks", // Use forks to ensure clean process state
  },
})
