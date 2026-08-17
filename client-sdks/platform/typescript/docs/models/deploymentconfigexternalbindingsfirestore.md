# DeploymentConfigExternalBindingsFirestore

GCP Firestore KV binding configuration

## Example Usage

```typescript
import { DeploymentConfigExternalBindingsFirestore } from "@alienplatform/platform-api/models";

let value: DeploymentConfigExternalBindingsFirestore = {
  service: "firestore",
  type: "kv",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `collectionName`                                                                                                     | *models.DeploymentConfigCollectionNameUnion*                                                                         | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `databaseId`                                                                                                         | *models.DeploymentConfigDatabaseIdUnion*                                                                             | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `projectId`                                                                                                          | *models.DeploymentConfigProjectIdUnion*                                                                              | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"firestore"*                                                                                                        | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.DeploymentConfigTypeKv2](../models/deploymentconfigtypekv2.md)                                               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |