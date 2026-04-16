## Writing Alien Bindings Tests

1. Follow existing test patterns - Look at `storage.rs`, `build.rs`, or `artifact_registry.rs` for complete examples of BindingsProvider testing
2. Use test context for proper resource cleanup - Implement `AsyncTestContext` with setup/teardown, track created resources for cleanup
3. Load .env.test - Always load environment variables from workspace root `.env.test` file: `dotenvy::from_path(workspace_root::get_workspace_root().join(".env.test"))`
4. Make tests concise - Prefer complete e2e lifecycle tests (create → use → delete) instead of many small tests
5. Generate unique resource names - Use UUIDs or timestamps to avoid test conflicts: `format!("alien-test-{}", uuid::Uuid::new_v4().simple())`
6. Test error scenarios comprehensively - Test not found (404), conflicts (409), access denied (403), and malformed requests
7. Implement robust wait patterns - Poll for resource readiness with timeouts, don't assume immediate availability
8. Handle graceful cleanup - Check for `RemoteResourceNotFound` errors during cleanup and continue silently
9. **Tests must be robust** - Use `.expect()` and `panic!` for operations that should succeed. No `warn!` for actual failures - tests should fail hard when things don't work
10. Test provider-specific patterns - Use `#[rstest]` with `#[case]` to verify different binding providers work (local, gRPC, cloud providers like AWS, GCP, Azure) without code duplication
11. Use BindingsProvider interface - Test through the unified BindingsProvider rather than provider-specific implementations
12. Test with real resources - All operations must work against real APIs/providers, no mocking in integration tests
13. Use proper environment variable names - Always use the correct env var names from .env.test (e.g., AWS_MANAGEMENT_REGION not AWS_REGION) and map them appropriately in test setup; copy from another binding test for consistency
14. Validate required environment variables - Provide clear error messages if required config is missing
