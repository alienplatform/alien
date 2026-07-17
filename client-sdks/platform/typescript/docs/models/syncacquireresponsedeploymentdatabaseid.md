# SyncAcquireResponseDeploymentDatabaseId

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentDatabaseId } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentDatabaseId = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                              | [models.SyncAcquireResponseDeploymentDatabaseIdSecretRef](../models/syncacquireresponsedeploymentdatabaseidsecretref.md) | :heavy_check_mark:                                                                                                       | Reference to a Kubernetes Secret                                                                                         |