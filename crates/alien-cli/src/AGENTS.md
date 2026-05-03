# alien-cli Guidelines

## Product Model

`alien-cli` is a plain CLI first.

- no TUI
- no dashboard entrypoint
- no `--no-tui` compatibility layer
- human terminals get readable progress and small bootstrap prompts
- automation gets flags, `--json`, and deterministic failures

Keep the command model clean:

- top-level `alien ...` manager commands target platform-managed or standalone managers
- `alien dev ...` targets the local manager only
- platform-only commands stay separate (`login`, `workspaces`, `projects`, `link`, `manager`)
- offline build commands stay separate from server-targeted commands

## Interaction Rules

Interactive prompts are allowed only as bootstrap help for humans in a real terminal.

Good examples:

- `alien login`
- `alien workspaces set`
- `alien link`
- manager-targeted commands that need to establish missing workspace/project context

Rules:

- every command must have a complete non-interactive path
- `--json` must never prompt
- non-interactive execution must fail fast with actionable guidance
- prompts should be simple line-oriented terminal prompts, not full-screen experiences

## Output Rules

When changing or adding commands:

- prefer clear phase-by-phase plain text for human mode
- support `--json` when the command returns structured data or is useful for tooling
- keep success summaries short and actionable
- end long-running flows with obvious next-step commands when that helps the user continue

Do not treat logs as an API. If tooling needs data, expose it explicitly with JSON output or a documented file contract.

## Local Dev Contract

`alien dev` is the shipped local workflow.

- bare `alien dev` starts the local manager, builds, creates a release, deploys, and waits
- `alien dev server` starts only the local manager
- local tooling uses `--status-file`
- the status file shape comes from `alien-core::DevStatus` and is generated into `@alienplatform/core`

If you change the dev status shape:

1. update `alien-core`
2. regenerate `alien/packages/core`
3. update any consumers such as `packages/testing`

Do not hand-maintain duplicate TypeScript copies of the dev status schema when the generated type can be imported.

## Error Handling

Use `alien-error` structured errors and add context at the boundary where information becomes actionable.

Preferred patterns:

```rust
operation().await.context(ErrorData::ApiRequestFailed {
    message: "creating release".to_string(),
    url: None,
})?;
```

```rust
return Err(AlienError::new(ErrorData::ValidationError {
    field: "project".to_string(),
    message: "run `alien link --project <name>` first".to_string(),
}));
```

Prefer explicit next steps over vague guidance.

## Documentation Expectations

If you change command behavior, update the public docs at <https://alien.dev/docs> at the same time. In particular, keep the CLI overview, local-development guide, and testing-framework docs in sync with the command model, bootstrap rules, and local machine interface.
