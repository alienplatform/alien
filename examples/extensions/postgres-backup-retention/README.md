# Keep Postgres backups longer

An [extension](https://alien.dev/docs/guide/extensions) that raises your Postgres database's backup retention to 30 days. It is two template modifiers, one for CloudFormation and one for Terraform, so every setup template your customers get carries the longer retention.

| Cloud | Template | Resource | Alien's value | After the extension |
|-------|----------|----------|---------------|---------------------|
| AWS | CloudFormation | `AWS::RDS::DBCluster` | `BackupRetentionPeriod: 7` | `30` |
| AWS | Terraform | `aws_rds_cluster` | `backup_retention_period = 7` | `30` |
| Google Cloud | Terraform | `google_sql_database_instance` | no retention, so Cloud SQL's default applies | `retained_backups = 30` |
| Azure | Terraform | `azurerm_postgresql_flexible_server` | `backup_retention_days = 7` | `30` |

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
import backups from "./backups/generated"

const db = new alien.Postgres("db").build()

export default new alien.Stack("my-app")
  .extensions([backups])
  .add(db, "frozen")
  .build()
```

## See it in the template

From your stack's directory, render each template and find the retention:

```bash
alien render --format cloudformation --stack alien.ts --json \
  | jq '.Resources[] | select(.Type == "AWS::RDS::DBCluster") | .Properties.BackupRetentionPeriod'
alien render --format terraform --target aws --stack alien.ts | grep backup_retention_period
alien render --format terraform --target gcp --stack alien.ts | grep -A 2 backup_retention_settings
alien render --format terraform --target azure --stack alien.ts | grep backup_retention_days
```

The first prints `30`, and each Terraform line shows `30`. `alien render` ends with the resources each extension changed.

Render with the `alien` CLI you install from [alien.dev](https://alien.dev/docs). A CLI built from this repository has no Postgres emitters, so it can't render a stack with Postgres.

The [backups example](https://alien.dev/docs/examples/postgres-backup-retention) walks through the modifiers.
