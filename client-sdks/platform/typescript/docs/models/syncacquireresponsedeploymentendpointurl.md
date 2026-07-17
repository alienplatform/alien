# SyncAcquireResponseDeploymentEndpointUrl

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentEndpointUrl } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentEndpointUrl = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                | [models.SyncAcquireResponseDeploymentEndpointUrlSecretRef](../models/syncacquireresponsedeploymentendpointurlsecretref.md) | :heavy_check_mark:                                                                                                         | Reference to a Kubernetes Secret                                                                                           |