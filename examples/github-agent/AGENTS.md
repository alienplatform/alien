# GitHub Deployment Example

- Use Hono for HTTP routes; export the app as default
- Use `@alienplatform/bindings` for commands and vault access
- Keep demo mode first-class (no real token required)
- Keep command handlers thin; call shared helpers for logic
- Store integration configs in the `integrations` vault; never log tokens
- Every command should have a matching test
- Use `@alienplatform/testing` with `method: "dev"` and assert both HTTP and command flows

## Don't

- Don't use "ARC" terminology — use "commands"
- Don't use "agent" terminology — use "deployment"
