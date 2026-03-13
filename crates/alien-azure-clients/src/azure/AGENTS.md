## Adding a New Azure Service Client

1. Auto-generate models: Go to https://github.com/Azure/azure-rest-api-specs, find your service specs (latest version), add to `scripts/download_azure_openapi_specs.sh`, add to `build.rs`, then models are available in `src/azure/models`
2. Follow existing patterns: Look at `container_apps.rs` for trait + client struct example
3. Validate against Azure API docs: Ensure request/response structs match exact field names, types, and required fields from Azure REST API Reference
4. Implement core operations only: OK to skip optional fields/features, but all required fields must be present for compatibility
5. Map service errors: Handle Azure error format and map to `RemoteResourceNotFound`, `AuthenticationError`, `RateLimitExceeded`
6. Use infrastructure: Use `AzureClientBase` for auth, handle long-running operations with polling, support `api-version` parameters
7. Add comprehensive tests: Create `tests/azure_[servicename]_client_tests.rs`, follow existing test patterns
