# SyncAcquireResponseDeploymentClusterEndpoint

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentClusterEndpoint } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentClusterEndpoint = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                        | [models.SyncAcquireResponseDeploymentClusterEndpointSecretRef](../models/syncacquireresponsedeploymentclusterendpointsecretref.md) | :heavy_check_mark:                                                                                                                 | Reference to a Kubernetes Secret                                                                                                   |