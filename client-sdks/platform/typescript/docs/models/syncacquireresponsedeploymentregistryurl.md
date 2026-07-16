# SyncAcquireResponseDeploymentRegistryUrl

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentRegistryUrl } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentRegistryUrl = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                | [models.SyncAcquireResponseDeploymentRegistryUrlSecretRef](../models/syncacquireresponsedeploymentregistryurlsecretref.md) | :heavy_check_mark:                                                                                                         | Reference to a Kubernetes Secret                                                                                           |