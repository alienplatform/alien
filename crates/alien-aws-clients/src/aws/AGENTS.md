## Adding a New AWS Service Client

1. Follow existing patterns: Look at `lambda.rs` for trait + client struct example
2. Validate against AWS API docs: Ensure request/response structs match exact field names, types, and required fields from AWS API Reference
3. Implement core operations only: OK to skip optional fields/features, but all required fields must be present for compatibility
4. Map service errors: Check AWS docs "Common Errors" section and map to `RemoteResourceNotFound`, `AuthenticationError`, `RateLimitExceeded`, etc
5. Use infrastructure: `.aws_sign_v4()` for auth, `.aws_error_for_status()` for errors, support `service_endpoint_overrides`
6. Add comprehensive tests: Create `tests/aws_[servicename]_client_tests.rs`, follow existing test patterns
