# SyncReconcileResponseExternalBindingsFirestore

GCP Firestore KV binding configuration

## Example Usage

```typescript
import { SyncReconcileResponseExternalBindingsFirestore } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseExternalBindingsFirestore = {
  service: "firestore",
  type: "kv",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `collectionName`                                                                                                     | *models.SyncReconcileResponseCollectionNameUnion*                                                                    | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `databaseId`                                                                                                         | *models.SyncReconcileResponseDatabaseIdUnion*                                                                        | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `projectId`                                                                                                          | *models.SyncReconcileResponseProjectIdUnion*                                                                         | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"firestore"*                                                                                                        | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncReconcileResponseTypeKv2](../models/syncreconcileresponsetypekv2.md)                                     | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |