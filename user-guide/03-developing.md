# Developing

```bash
alien dev
```

This is the local Alien workflow. It starts the local manager, builds your app for the local platform, creates a local release, creates or updates the initial deployment, and prints the local URLs you can use.

`alien dev` is local-only. For non-local managers, use the top-level commands such as `alien release`, `alien deploy`, and `alien deployments ...`.

## Server Only

If you only want the local manager:

```bash
alien dev server
```

Then you can drive it with explicit local commands:

```bash
alien dev deployments ls
alien dev release
alien dev deploy --name preview
```

## Machine Interface

For tooling, use the status file contract instead of parsing terminal output:

```bash
alien dev --status-file .alien/dev-status.json
```

## Next

- [Onboarding Customers](04-onboarding.md)
