import { defineConfig } from "vitest/config"

/**
 * Dedicated real-wire integration config (`test:integration`). Runs ONLY the
 * suite that drives the actual Rust command server. The hook timeout covers a
 * cold `cargo build` of the `test-command-server` bin in `beforeAll`.
 *
 * `pool: "forks"` is load-bearing, not cosmetic: each test spawns a real
 * `test-command-server` child and exercises real `fetch` sockets against it.
 * Even with careful teardown, an abandoned handler timer (the budget test
 * deliberately outlives its lease) or an undici keep-alive socket can leave
 * the vitest `threads` worker's event loop busy enough that the worker↔main
 * RPC (`onTaskUpdate`) misses its deadline — surfacing as a spurious
 * `[vitest-worker]: Timeout calling "onTaskUpdate"` unhandled error even
 * though every test already passed. Running the file in its own forked
 * process sidesteps that: the process hard-exits after the run instead of
 * relying on the shared worker thread staying responsive.
 */
export default defineConfig({
  test: {
    include: ["tests/integration.real-server.test.ts"],
    hookTimeout: 600_000,
    testTimeout: 30_000,
    teardownTimeout: 10_000,
    pool: "forks",
  },
})
