# SyncAcquireResponseExternalBindingsFirestore

GCP Firestore KV binding configuration

## Example Usage

```typescript
import { SyncAcquireResponseExternalBindingsFirestore } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseExternalBindingsFirestore = {
  service: "firestore",
  type: "kv",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `collectionName`                                                                                                     | *models.SyncAcquireResponseCollectionNameUnion*                                                                      | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `databaseId`                                                                                                         | *models.SyncAcquireResponseDatabaseIdUnion*                                                                          | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `projectId`                                                                                                          | *models.SyncAcquireResponseProjectIdUnion*                                                                           | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"firestore"*                                                                                                        | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseTypeKv2](../models/syncacquireresponsetypekv2.md)                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |