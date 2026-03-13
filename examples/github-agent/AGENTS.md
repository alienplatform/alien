# GitHub Agent Example - Guidelines

- Use Hono for HTTP routes; export the app as default
- Use `@aliendotdev/bindings` for ARC commands and vault access
- Keep demo mode first-class (no real token required)
- Keep command handlers thin; call shared helpers for logic
- Store integration configs in the `integrations` vault; never log tokens
- Every ARC command should have a matching test
- Use `@aliendotdev/testing` with `method: "dev"` and assert both HTTP and ARC flows
