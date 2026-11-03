# alien-stack-toml

`alien.toml` is a declarative way to write an Alien stack, for projects that don't otherwise use TypeScript. It uses the same resources and names as `alien.ts` and resolves to the same stack. The CLI parses it itself, so loading it runs no code and needs no Bun, Node or package manager.

```toml
#:schema https://alien.dev/schemas/alien.toml.json
name = "acme-agent"
platforms = ["machines"]

[daemon.agent]
build = { rust = "acme-agent" }
health = { port = 8080, path = "/health" }
```

`alien build`, `alien dev`, `alien release` and `alien render` find `alien.toml` in the project directory. When `alien.ts`, `alien.js` or `alien.json` is also there, that file wins and the CLI warns, so adding an `alien.toml` can't change what an existing project releases.

Unknown keys are errors. Every error names the file, line and column:

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

All pools belong to one compute cluster. On machines deployments the pool name is what `join --capacity-group` takes.

| Key | Meaning |
|---|---|
| `cpu` | Cores per machine: `4`, `0.5` or `"8"` (required). |
| `memory` | Memory per machine, such as `"16Gi"` (required). |
| `disk` | Size of the disk holding the operating system. Defaults to `"20Gi"`. |
| `data-disk` | An extra empty disk: `"1Ti"`, or a range each deployment picks from, `{ min = "0Gi", default = "200Gi", max = "4Ti" }`. |
| `arch` | `"x86_64"` or `"arm64"`. |
| `nested-virtualization` | Machines must support nested virtualization. |
| `gpu` | `{ type = "nvidia-l4", count = 1 }`. |
| `machines` | Fixed count: `2`, or `{ min = 1, max = 5, default = 2 }`. |
| `autoscale` | `{ min = 1, max = 10 }`; each bound may also be a range. Set `machines` or `autoscale`. |
| `failure-domain-spread` | Zones to spread across, 1 to 255. |

Quantities take `Ki`, `Mi`, `Gi`, `Ti` (powers of 1024) or `k`, `M`, `G`, `T` (powers of 1000).

### `[daemon.<id>]`

| Key | Meaning |
|---|---|
| `build` | Build from source: `{ rust = "<binary>", src = "." }`. |
| `image` | Run a prebuilt image instead of `build`. |
| `pool` | Pool to run on. Omit to run on every pool. |
| `cpu`, `memory` | Reserved per instance. Default `0.1` and `"128Mi"`. |
| `command` | Command overriding the image's default. |
| `host-access` | `true` runs the daemon privileged, in the host's PID namespace and network, with the host's `/` at `/host`, as uid 0. |
| `stop-grace-period` | Seconds, or `"10m"`, `"24h"`, `"1h30m"`. At most 24 hours. |
| `health` | `{ port = 8080, path = "/health", method = "GET", timeout = "2s", failure-threshold = 3 }`. `port` is required. |
| `env` | Environment variables. |
| `env-file` | A `KEY=value` file relative to `alien.toml`, read when the stack loads. `env` wins over it. |
| `links` | IDs of resources the daemon uses. |
| `permissions` | Permission profile that gives the daemon a cloud identity. On AWS, GCP and Azure it can't be combined with `host-access`. |
| `lifecycle` | `"live"` (default) or `"frozen"`. |
| `platforms` | Create the daemon only on these platforms. |
| `enabled-when` | Create the daemon only when this deployer boolean `[input.<id>]` is true. Set this or `platforms`. |

`[daemon.<id>.endpoints]` declares public HTTP endpoints by name: `api = { port = 8080, host = "@", wildcard = false }`. `host` is the label under the deployment's domain (`"@"` is the domain itself) and defaults to the endpoint name.

`[daemon.<id>.secrets]` declares secrets by environment variable name. Each becomes a stack input the developer provides, required unless `required = false`, set as that variable on this daemon: `AUTH_KEY = { label = "Auth key", description = "...", required = true, platforms = ["machines"] }`. Its input ID is the name in camelCase (`authKey`). Declaring the same secret, with the same settings, on several daemons sets it on each.

The env file format: one `KEY=value` per line, `#` starts a comment line, whitespace around keys and values is trimmed, `"double"` quotes allow `\"`, `\\` and `\n`, `'single'` quotes are literal. There are no inline comments.

### `[storage.<id>]`

| Key | Meaning |
|---|---|
| `public-read` | Anyone may read objects. |
| `versioning` | Keep previous object versions. |
| `lifecycle` | `"frozen"` (default) or `"live"`. |
| `platforms` | Create the bucket only on these platforms. |
| `enabled-when` | Create the bucket only when this deployer boolean `[input.<id>]` is true. Set this or `platforms`. |

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
| `env` | Variables that receive the value: `"NAME"`, `{ name = "NAME", targets = ["daemon-id"] }`, or a list. Without `targets`, every daemon gets it. |
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

### Platform scope

`platforms` on a daemon or bucket is carried by the mechanism stacks already use: a deployer boolean input, `<id>Enabled` (default `true`), that exists only on those platforms and gates the resource. Elsewhere the resource is never created. On the listed platforms, the deployer can still decline it.

## Editor support

The JSON schema in [`schema/alien.toml.json`](schema/alien.toml.json) is generated from this crate's types. With Taplo or Even Better TOML, the `#:schema` comment on the first line enables completion and validation. Regenerate it with `ALIEN_UPDATE_SCHEMA=1 cargo test -p alien-stack-toml --test schema`.

## Tests

`tests/fixtures/<case>/` pairs an `alien.toml` with the equivalent `alien.ts`. `tests/fixtures/regenerate.sh` evaluates each `alien.ts` with the TypeScript SDK in this repository and writes `expected.json`. `tests/differential.rs` requires the TOML side to resolve to the same stack.
