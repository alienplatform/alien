## Adding a New GCP Service Client

1. Follow existing patterns: Look at `cloudrun.rs` for trait + client struct example
2. Validate against GCP API docs: Ensure request/response structs match exact field names, types, and required fields from Google Cloud API Reference
3. Implement core operations only: OK to skip optional fields/features, but all required fields must be present for compatibility
4. Map service errors: Handle standard GCP error format (`error.code`, `error.message`) and map to `RemoteResourceNotFound`, `AuthenticationError`, `RateLimitExceeded`, etc.
5. Use infrastructure: `.gcp_auth()` for auth, `.gcp_error_for_status()` for errors, use `longrunning.rs` for async operations
6. Add comprehensive tests: Create `tests/gcp_[servicename]_client_tests.rs` with GCP emulator support when available, follow existing test patterns
