## Adding a New Kubernetes Resource Client

1. Follow existing patterns: Look at `deployments.rs` for trait + client struct example
2. Validate against Kubernetes API docs: Ensure request/response structs match exact field names, types, and required fields from Kubernetes API Reference
3. Implement core operations only: OK to skip optional fields/features, but all required fields must be present for compatibility
4. Map service errors: Handle Kubernetes error format and map to `RemoteResourceNotFound`, `AuthenticationError`, `RateLimitExceeded`, etc.
5. Use infrastructure: Use `KubernetesClient` base, handle authentication via kubeconfig/service accounts, support namespace scoping
6. Add comprehensive tests: Create appropriate test files, test with different auth methods, follow existing test patterns
