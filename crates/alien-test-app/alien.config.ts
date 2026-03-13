import { defineFunction, kv, storage, queue } from "@aliendotdev/core";

export default defineFunction({
  name: "alien-test-app",
  runtime: "rust",
  handler: "alien-test-app",
  bindings: [
    // KV for storing event verification data
    kv({
      name: "test-kv",
    }),
    // Storage for testing storage events
    storage({
      name: "test-storage",
      subscriptions: ["*"], // Subscribe to all object events
    }),
    // Queue for testing queue message handling
    queue({
      name: "test-queue",
    }),
  ],
});



