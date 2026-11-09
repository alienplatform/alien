# alien-stack-toml

`alien.toml` is a declarative way to write an Alien stack, for projects that don't otherwise use TypeScript. It uses the same resources and names as `alien.ts` and resolves to the same stack. The CLI parses it itself, so loading it runs no code and needs no Bun, Node or package manager.

```toml
#:schema https://alien.dev/schemas/alien.toml.json
name = "acme-agent"
platforms = ["machines"]

[daemon.agent]
build = { toolchain = "rust", binary-name = "acme-agent" }
health-check = { port = 8080, path = "/health" }
```

`alien build`, `alien dev`, `alien release` and `alien render` find `alien.toml` in the project directory. When `alien.ts`, `alien.js` or `alien.json` is also there, that file wins and the CLI warns, so adding an `alien.toml` can't change what an existing project releases.

Unknown keys are errors, with a "did you mean" hint when one is close to a real key. Every error names the file, line and column:

```
alien.toml:12:8: unknown pool 'gpu'; declared pools: general
   |
12 | pool = "gpu"
   |        ^^^^^
```

## Reference

Keys are kebab-case. Tables are keyed by ID: `[daemon.loader]` declares the daemon `loader`. IDs are shared across resource types.

### Top level

| Key | Meaning |
|---|---|
| `name` | Stack name (required). Letters, digits, `-` and `_`. |
| `platforms` | Platforms the stack deploys to: `aws`, `gcp`, `azure`, `kubernetes`, `machines`, `local`. Omit for all. |
| `cluster` | ID of the compute cluster that holds the pools. Defaults to `compute`. Set it to keep the ID of an existing stack's cluster. |

### `[pool.<name>]`

All pools belong to one compute cluster. On machines deployments the pool name is what `join --pool` takes.

| Key | Meaning |
|---|---|
| `cpu` | Cores per machine: `4` or `0.5` (required). |
| `memory` | Memory per machine, such as `"16Gi"` (required). |
| `ephemeral-storage` | Size of the disk holding the operating system. Defaults to `"20Gi"`. |
| `data-disk` | An extra empty disk: `"1Ti"`, or a range each deployment picks from, `{ default = "200Gi", allowed = ["0Gi", "4Ti"] }`. |
| `architecture` | `"x86_64"` or `"arm64"`. |
| `nested-virtualization` | Machines must support nested virtualization. |
| `gpu` | `{ type = "nvidia-l4", count = 1 }`. |
| `machines` | Fixed count: `2`, or a range the deployer picks from, `{ default = 2, allowed = [1, 5] }`. |
| `autoscaling` | `{ min = 1, max = 10 }`; each bound may also be a range. Set `machines` or `autoscaling`. |
| `failure-domain-spread` | Zones to spread across, 1 to 255. |

Quantities take `Ki`, `Mi`, `Gi`, `Ti` (powers of 1024).

### `[daemon.<id>]`

| Key | Meaning |
|---|---|
| `build` | Build from source: `{ toolchain = "rust", binary-name = "<binary>", src = "." }`. `toolchain` is `rust`, `typescript`, `python` or `docker`; `docker` also takes `dockerfile`, `target` and `build-args`, and `python` takes `command`, `python-version` and `package`. Programs in other languages use `docker`. |
| `image` | Run a prebuilt image instead of `build`. |
| `pool` | Pool to run on. Omit to run on every pool. |
| `cpu`, `memory` | Reserved per instance. Default `0.1` and `"128Mi"`. |
| `command` | Command overriding the image's default. |
| `host-access` | `true` runs the daemon privileged, in the host's PID namespace and network, with the host's `/` at `/host`, as uid 0. |
| `stop-grace-period` | Seconds, or `"10m"`, `"24h"`, `"1h30m"`. At most 24 hours. |
| `health-check` | `{ port = 8080, path = "/health", method = "GET", timeout = "2s", failure-threshold = 3 }`. `port` is required. |
| `env` | Environment variables: a literal, or `{ input = "<id>" }` to take the value of a stack input. |
| `env-file` | A `KEY=value` file, or a list of them, relative to `alien.toml`, read when the stack loads. Later files win over earlier ones, and `env` wins over all. |
| `links` | IDs of resources the daemon uses. |
| `permissions` | Gives the daemon a cloud identity: a profile name, or the grants in place, `{ files = ["storage/data-read"] }`, which make a profile named after the daemon. Omit for none. On AWS, GCP and Azure it can't be combined with `host-access`. |
| `lifecycle` | `"live"` (default) or `"frozen"`. |
| `enabled-when` | Create the daemon only when this deployer boolean `[input.<id>]` is true. |

`[daemon.<id>.public-endpoints]` declares public HTTP endpoints by name: `api = { port = 8080, host-label = "@", wildcard-subdomains = false }`. `host-label` is the label under the deployment's domain (`"@"` is the domain itself) and defaults to the endpoint name.

`[daemon.<id>.env]` holds the daemon's environment. A value written `{ input = "api_token" }` takes the value of the stack input `api_token`; the variable name must be uppercase. The same input under the same name on several daemons becomes one mapping with all of them as targets. A lowercase key that is also a setting of the daemon (such as `memory`) is rejected: it was most likely meant to sit above the `[daemon.<id>.env]` line.

The env file format: one `KEY=value` per line, `#` starts a comment line, whitespace around keys and values is trimmed, `"double"` quotes allow `\"`, `\\` and `\n`, `'single'` quotes are literal. There are no inline comments.

### `[storage.<id>]`

| Key | Meaning |
|---|---|
| `public-read` | Anyone may read objects. |
| `versioning` | Keep previous object versions. |
| `lifecycle` | `"frozen"` (default) or `"live"`. |
| `enabled-when` | Create the bucket only when this deployer boolean `[input.<id>]` is true. |

### `[input.<id>]`

A stack input: a value provided per deployment. The table name is the input ID.

| Key | Meaning |
|---|---|
| `kind` | `string`, `secret`, `number`, `integer`, `boolean`, `enum` or `string-list` (required). |
| `provided-by` | `"developer"` (default), `"deployer"`, or both. |
| `required` | Defaults to `true`. |
| `label`, `description`, `placeholder` | Shown where the value is asked for. `label` defaults to the ID; required inputs need a `description`. |
| `default` | Value used when none is provided. |
| `platforms` | Ask only on these platforms. |
| `env` | Variables that receive the value: `"NAME"`, `{ name = "NAME", target-resources = ["daemon-id"] }`, or a list. Without `target-resources`, every daemon gets it. Most inputs are wired from the daemon side instead, with `NAME = { input = "<id>" }` in `env`. |
| `min-length`, `max-length`, `pattern` | String and secret constraints. |
| `min`, `max` | Number and integer constraints. |
| `values` | The values an enum accepts. |
| `min-items`, `max-items` | String-list constraints. |

### `[permissions.<profile>]`

A permission profile maps resource IDs, or `"*"` for every resource, to the permission sets it grants:

```toml
[permissions.loader]
snapshots = ["storage/data-read", "storage/data-write"]
```

## Inspecting the resolved stack

`alien stack expand [path]` prints the stack an `alien.toml` resolves to as JSON, with every default and expansion made visible, and nothing else on stdout. `path` is an `alien.toml` or a directory that holds one, and defaults to the current directory. Errors name the file, line and column.

## Editor support

[`schema/alien.toml.json`](schema/alien.toml.json) is a JSON schema in the spelling authors use (kebab-case keys, `"2m"` and `"512Mi"` for durations and sizes, the shorthand forms). With Taplo or Even Better TOML, the `#:schema` comment on the first line enables completion and validation. It is generated from the same type definitions the resolver reads, and a test fails when the checked-in file is stale. Regenerate it with `UPDATE_SCHEMA=1 cargo test -p alien-stack-toml --test schema`.

## Tests

- `tests/fixtures/<case>/` pairs an `alien.toml` with the equivalent `alien.ts`. `tests/fixtures/regenerate.sh [case...]` evaluates each `alien.ts` with the TypeScript SDK in this repository and writes `expected.json`. `tests/differential.rs` requires the TOML side to resolve to the same stack.
- `tests/coverage.rs` requires every field of the editor schema to be written by at least one fixture.
- `tests/editor_schema.rs` checks every fixture against the editor schema, and that the schema rejects what the resolver rejects.
- `tests/errors.rs` checks the position and wording of errors.
- `tests/expand.rs` checks that `expand` returns the loaded stack, as JSON, for every fixture.
