# SyncAcquireResponseDeploymentPushRoleArn

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPushRoleArn } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPushRoleArn = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                | [models.SyncAcquireResponseDeploymentPushRoleArnSecretRef](../models/syncacquireresponsedeploymentpushrolearnsecretref.md) | :heavy_check_mark:                                                                                                         | Reference to a Kubernetes Secret                                                                                           |