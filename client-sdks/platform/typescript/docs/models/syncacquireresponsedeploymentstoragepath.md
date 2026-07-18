# SyncAcquireResponseDeploymentStoragePath

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentStoragePath } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentStoragePath = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                | [models.SyncAcquireResponseDeploymentStoragePathSecretRef](../models/syncacquireresponsedeploymentstoragepathsecretref.md) | :heavy_check_mark:                                                                                                         | Reference to a Kubernetes Secret                                                                                           |