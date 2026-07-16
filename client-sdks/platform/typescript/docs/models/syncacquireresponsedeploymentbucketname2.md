# SyncAcquireResponseDeploymentBucketName2

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentBucketName2 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentBucketName2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                | [models.SyncAcquireResponseDeploymentBucketNameSecretRef2](../models/syncacquireresponsedeploymentbucketnamesecretref2.md) | :heavy_check_mark:                                                                                                         | Reference to a Kubernetes Secret                                                                                           |