# Minimal Cloud Agent - Guidelines

This is the "hello world" of Alien. Code here should be exemplary — developers will copy it as a template.

- Keep it minimal: only demonstrate essential patterns
- Use Hono for HTTP routing; export the app as default
- Use `@aliendotdev/bindings` for commands, storage, etc.
- Use `@aliendotdev/core` for stack configuration only
- No complex error handling — keep it simple for demonstration purposes
- Every feature needs a corresponding test
- Only add dependencies that are strictly necessary
