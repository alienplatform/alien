# Add your tags to Alien's resources

An [extension](https://alien.dev/docs/guide/extensions) that adds your tags to the resources Alien's controllers create. It is a set of interceptors, one per cloud, that add `team` and `managed-by` to each create call before Alien signs it.

| Interceptor | Matches | Writes |
|-------------|---------|--------|
| `RdsTags` | RDS `CreateDBCluster` | `Tags.Tag.N.Key` and `Value` form members |
| `DynamoDbTags` | DynamoDB `CreateTable` | the `Tags` list in the JSON body |
| `CloudSqlLabels` | Cloud SQL `POST projects/*/instances` | `settings.userLabels` |
| `AzureTags` | `PUT` on Postgres flexible servers and storage accounts | `tags` |
| `KubernetesLabels` | every Kubernetes create | `metadata.labels` |

A frozen resource the setup template creates goes through no Alien call. Tag it with a template modifier, as the [backups example](../postgres-backup-retention) does.

## Test and build

You need the Rust target the hooks build for:

```bash
rustup target add wasm32-wasip1
```

```bash
alien extensions test
alien extensions build
```

`alien extensions test` runs `adds_tags_after_alien_tags` and the other tests natively and again inside the sandbox Alien runs hooks in, and fails if a result differs. `alien extensions build` writes the bundle to `target/alien-extension/` and `generated/index.ts`, the module your stack imports.

## In your stack

```ts
import * as alien from "@alienplatform/core"
import tags from "./tags/generated"

const db = new alien.Postgres("db").build()

export default new alien.Stack("my-app")
  .extensions([tags])
  .add(db, "live")
  .build()
```

## Where to see it

Interceptors change Alien's calls, not the setup templates: `alien render` prints the same template with or without the extension. The tags show on the resource after Alien's controller creates it, in the cloud's own listing:

```bash
aws rds describe-db-clusters --db-cluster-identifier <cluster-id> --query 'DBClusters[0].TagList'
```

The list holds Alien's tags and `team` and `managed-by`. The other clouds show them as labels on the Cloud SQL instance (`gcloud sql instances describe`), as tags on the Azure resource (`az resource show`), and as labels on the Kubernetes object (`kubectl get --show-labels`).

The [tags example](https://alien.dev/docs/examples/resource-tags) walks through each interceptor.
