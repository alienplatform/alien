# Turn on IAM database authentication

An [extension](https://alien.dev/docs/guide/extensions) that turns on IAM database authentication for the Postgres database Alien creates, on AWS, Google Cloud and Azure.

Password login keeps working, so Alien's own connection to the database does too.

| Cloud | Who creates the database | Hook | Setting |
|-------|--------------------------|------|---------|
| AWS | the setup template | CloudFormation modifier | `EnableIAMDatabaseAuthentication: true` on `AWS::RDS::DBCluster` |
| AWS | the setup template | Terraform modifier | `iam_database_authentication_enabled = true` on `aws_rds_cluster` |
| Google Cloud | the setup template | Terraform modifier | a `database_flags` block with `cloudsql.iam_authentication = on` in the instance's `settings` |
| Azure | the setup template | Terraform modifier | an `authentication` block on `azurerm_postgresql_flexible_server`, with the tenant from `data.azurerm_client_config` |
| AWS | Alien's controllers | interceptor | `EnableIAMDatabaseAuthentication` on `CreateDBCluster` and `ModifyDBCluster` |
| Google Cloud | Alien's controllers | interceptor | `settings.databaseFlags` on `POST projects/*/instances` |
| Azure | Alien's controllers | interceptor | `properties.authConfig` on the server `PUT`, with the call's tenant |

## Test and build

You need the Rust target the hooks build for:

```bash
rustup target add wasm32-wasip1
```

```bash
alien extensions test
alien extensions build
```

`alien extensions test` runs the hooks crate's tests natively and again inside the sandbox Alien runs hooks in, and fails if a result differs. `alien extensions build` writes the bundle to `target/alien-extension/` and `generated/index.ts`, the module your stack imports.

## In your stack

```ts
import * as alien from "@alienplatform/core"
import postgresIamAuth from "./postgres-iam-auth/generated"

const db = new alien.Postgres("db").build()

export default new alien.Stack("my-app")
  .extensions([postgresIamAuth])
  .add(db, "frozen")
  .build()
```

## See it in the template

From your stack's directory:

```bash
alien render --format cloudformation --stack alien.ts | grep EnableIAMDatabaseAuthentication
alien render --format terraform --target aws --stack alien.ts | grep iam_database_authentication_enabled
alien render --format terraform --target gcp --stack alien.ts | grep -A 1 'cloudsql.iam_authentication'
alien render --format terraform --target azure --stack alien.ts | grep -A 4 'authentication {'
```

`alien render` also lists which extension changed which resource. On Azure the module gains `postgres-iam-auth.tf` with the `azurerm_client_config` data source the tenant comes from.

Render with the `alien` CLI you install from [alien.dev](https://alien.dev/docs). A CLI built from this repository has no Postgres emitters, so it can't render a stack with Postgres.

The interceptors change no template. They run in the create calls of a live database, and of a frozen one the admin creates with the setup CLI, and the flag shows in the cloud's describe output (`aws rds describe-db-clusters`, `gcloud sql instances describe`, `az postgres flexible-server show`).

The [IAM database authentication example](https://alien.dev/docs/examples/postgres-iam-auth) walks through the modifiers and interceptors.
