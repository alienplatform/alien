# SyncAcquireResponseDeploymentResourceId

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentResourceId } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentResourceId = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                              | [models.SyncAcquireResponseDeploymentResourceIdSecretRef](../models/syncacquireresponsedeploymentresourceidsecretref.md) | :heavy_check_mark:                                                                                                       | Reference to a Kubernetes Secret                                                                                         |