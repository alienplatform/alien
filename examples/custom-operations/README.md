# Custom operations

This Rust plugin exposes two operations against an application's HTTP admin API:

- `custom-ops/doctor` reads health and the current export limit.
- `custom-ops/throttle-export-automation` changes that limit. Its mutating risk tier lets Remote Operator apply the project's approval policy.

The `TypedOperations` registry owns dispatch, parameter validation, input and output
schemas, risk, permissions, and timeouts. `metadata.json` comes from that registry.
There is no operation-name switch or separately maintained schema.

## Run and verify

From the repository root:

```bash
cargo run -p custom-ops --bin generate-metadata
cargo run -p custom-ops --bin generate-metadata -- --check
cargo nextest run -p custom-ops
alien operations check examples/custom-operations
```

The tests start an HTTP application on a random loopback port, invoke the compiled
plugin through stdin/stdout, change its export throttle, and read it back. Invalid
parameters, unknown operations, and unsupported protocol versions must leave the
application untouched. Tests also compare published metadata with the registry.

## Application contract

Set `CUSTOM_OPS_BASE_URL` in the plugin's process environment to an admin API URL
reachable from Remote Operator. The example uses network access to the application;
it requests no cloud IAM or Kubernetes API permissions. It expects an API without
HTTP authentication. Add your application's authentication to the HTTP client when
adapting the example. Keep credentials in plugin configuration, outside caller params.

`GET /health` returns:

```json
{"status":"healthy","maxExportsPerMinute":100}
```

`PUT /admin/export-throttle` accepts and returns:

```json
{"maxExportsPerMinute":12}
```

The limit is a positive `u32`. Both runtime decoding and generated JSON Schema reject
zero. The HTTP client times out after ten seconds and does not follow redirects.
The service must finish applying the new limit before returning its response.

## Build and invoke

On a Linux builder, run `alien operations package examples/custom-operations` to
build the executable for that host architecture and produce a bundle. Publish the
bundle with `alien operations publish`, then enable the plugin for your project.
Build a bundle for each required Linux architecture. `alien operations check` and
packaging verify generated metadata before proceeding.

After installation, use the Operations CLI:

```bash
alien operations invoke --deployment acme/prod --operation custom-ops/doctor
alien operations invoke --deployment acme/prod \
  --operation custom-ops/throttle-export-automation \
  --params '{"maxExportsPerMinute":12}'
```

Application RPC handlers use `alien commands invoke`. Custom infrastructure plugins
run through `alien operations invoke` so their declared risk and project policy apply.
