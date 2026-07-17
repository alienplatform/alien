# SyncAcquireResponseDeploymentPullRoleArn

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPullRoleArn } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPullRoleArn = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                | [models.SyncAcquireResponseDeploymentPullRoleArnSecretRef](../models/syncacquireresponsedeploymentpullrolearnsecretref.md) | :heavy_check_mark:                                                                                                         | Reference to a Kubernetes Secret                                                                                           |