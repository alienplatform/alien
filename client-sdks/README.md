# TypeScript API SDKs

Both TypeScript API clients use the shared Speakeasy overlay in
`speakeasy-overlay.yaml`. Operations with a request body accept at most two
arguments before the optional request options:

- A body-only operation accepts the body directly.
- An operation with one other parameter accepts `(parameter, body)`.
- Operations with more parameters keep their named request object.
- Operations without a request body keep their existing signatures.

The HTTP APIs and model types are unchanged. The new signatures are a breaking
SDK change; migrate callers when upgrading to the next major release.

```ts
// Platform API
await alien.projects.update("my-project", { name: "renamed-project" })
await alien.deploymentGroups.createToken(group.id, {
  description: "Deployment setup",
})

// Manager API
await manager.deployments.setDeploymentChannel(deploymentId, {
  channel: "stable",
})
```

Configure Platform API workspace selection once on the client:

```ts
const alien = new Alien({ apiKey, workspace: "my-workspace" })
```

Standalone functions follow the same convention, with the client as the first
argument. Generated React Query hooks continue to accept their generated
variable types.

Regenerate from the checked-in contracts with `pnpm generate:platform-api` and
`pnpm generate:manager-api`. The scripts validate that the shared overlay
matches endpoints, build the generated packages, and run request serialization
and response validation tests. They require the pinned Speakeasy CLI versions
specified in each script; set `SPEAKEASY_BIN` to the corresponding executable.
