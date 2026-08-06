# SyncAcquireResponseDeploymentServerCaCertificates

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentServerCaCertificates } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentServerCaCertificates = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                                        | Type                                                                                                                                         | Required                                                                                                                                     | Description                                                                                                                                  |
| -------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                                  | [models.SyncAcquireResponseDeploymentServerCaCertificatesSecretRef](../models/syncacquireresponsedeploymentservercacertificatessecretref.md) | :heavy_check_mark:                                                                                                                           | Reference to a Kubernetes Secret                                                                                                             |