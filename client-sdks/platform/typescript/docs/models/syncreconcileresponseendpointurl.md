# SyncReconcileResponseEndpointUrl

## Example Usage

```typescript
import { SyncReconcileResponseEndpointUrl } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseEndpointUrl = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                | [models.SyncReconcileResponseEndpointUrlSecretRef](../models/syncreconcileresponseendpointurlsecretref.md) | :heavy_check_mark:                                                                                         | Reference to a Kubernetes Secret                                                                           |