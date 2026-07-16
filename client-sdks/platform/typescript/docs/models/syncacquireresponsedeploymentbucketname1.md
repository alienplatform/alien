# SyncAcquireResponseDeploymentBucketName1

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentBucketName1 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentBucketName1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                | [models.SyncAcquireResponseDeploymentBucketNameSecretRef1](../models/syncacquireresponsedeploymentbucketnamesecretref1.md) | :heavy_check_mark:                                                                                                         | Reference to a Kubernetes Secret                                                                                           |