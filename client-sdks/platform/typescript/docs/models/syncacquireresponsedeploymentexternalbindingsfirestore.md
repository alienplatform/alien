# SyncAcquireResponseDeploymentExternalBindingsFirestore

GCP Firestore KV binding configuration

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentExternalBindingsFirestore } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentExternalBindingsFirestore = {
  service: "firestore",
  type: "kv",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `collectionName`                                                                                                     | *models.SyncAcquireResponseDeploymentCollectionNameUnion*                                                            | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `databaseId`                                                                                                         | *models.SyncAcquireResponseDeploymentDatabaseIdUnion*                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `projectId`                                                                                                          | *models.SyncAcquireResponseDeploymentProjectIdUnion*                                                                 | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"firestore"*                                                                                                        | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseDeploymentTypeKv2](../models/syncacquireresponsedeploymenttypekv2.md)                     | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |