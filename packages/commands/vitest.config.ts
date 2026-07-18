import { configDefaults, defineConfig } from "vitest/config"

/**
 * Default unit-suite config (`test` / `test:bun`). The real-wire integration
 * suite is excluded here — it requires cargo + the Rust command server and runs
 * only via the dedicated `test:integration` script (see `vitest.integration.config.ts`).
 */
export default defineConfig({
  test: {
    exclude: [...configDefaults.exclude, "tests/integration.real-server.test.ts"],
  },
})
