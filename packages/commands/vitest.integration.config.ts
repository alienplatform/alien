import { defineConfig } from "vitest/config"

/**
 * Dedicated real-wire integration config (`test:integration`). Runs ONLY the
 * suite that drives the actual Rust command server. The hook timeout covers a
 * cold `cargo build` of the `test-command-server` bin in `beforeAll`.
 */
export default defineConfig({
  test: {
    include: ["tests/integration.real-server.test.ts"],
    hookTimeout: 600_000,
    testTimeout: 30_000,
  },
})
