# SyncAcquireResponseDeploymentCollectionName

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentCollectionName } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentCollectionName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                      | [models.SyncAcquireResponseDeploymentCollectionNameSecretRef](../models/syncacquireresponsedeploymentcollectionnamesecretref.md) | :heavy_check_mark:                                                                                                               | Reference to a Kubernetes Secret                                                                                                 |