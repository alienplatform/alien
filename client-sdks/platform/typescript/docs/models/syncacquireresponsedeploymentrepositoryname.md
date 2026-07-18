# SyncAcquireResponseDeploymentRepositoryName

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentRepositoryName } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentRepositoryName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                      | [models.SyncAcquireResponseDeploymentRepositoryNameSecretRef](../models/syncacquireresponsedeploymentrepositorynamesecretref.md) | :heavy_check_mark:                                                                                                               | Reference to a Kubernetes Secret                                                                                                 |