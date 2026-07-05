# SyncReconcileResponseClusterEndpoint

## Example Usage

```typescript
import { SyncReconcileResponseClusterEndpoint } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseClusterEndpoint = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                        | [models.SyncReconcileResponseClusterEndpointSecretRef](../models/syncreconcileresponseclusterendpointsecretref.md) | :heavy_check_mark:                                                                                                 | Reference to a Kubernetes Secret                                                                                   |