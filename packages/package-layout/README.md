# `@alienplatform/package-layout`

Executable consumer test for the public TypeScript packages. It tests the
published artifact instead of workspace symlinks:

1. `pnpm pack` SDK, core, bindings, and commands.
2. Install those tarballs into a throwaway npm consumer.
3. Import the documented surfaces under Node and Bun.
4. Typecheck against the shipped declarations.
5. Reject missing or unexpected packed files (`files: ["dist"]` keeps
   workspace-only sources and contract notes out of the tarballs).
6. Compile and run a Bun executable with the native bindings addon embedded.

The root command first runs the small static package-boundary guard in
`packages/scripts/validate-package-layout.ts`; behavioral checks stay here.

Run it from the repository root:

```sh
pnpm validate:package-layout
```

Or run the fixture directly:

```sh
pnpm --filter @alienplatform/package-layout check
```

Node 22+, Bun, npm, pnpm, Cargo, and network access for non-Alien npm
dependencies are required. The fixture builds a host bindings addon when a
local or prebuilt addon is unavailable. A failed operation fails the command;
there is no expected-failure allowlist.

Generated directories (`.tarballs`, `fixture/node_modules`,
`fixture/.compiled`) are ignored.
The runner restores the committed fixture manifest after every run, so package
version bumps do not leave validation-only diffs behind.
