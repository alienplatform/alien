# SyncReconcileResponseRegistryUrl

## Example Usage

```typescript
import { SyncReconcileResponseRegistryUrl } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseRegistryUrl = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                | [models.SyncReconcileResponseRegistryUrlSecretRef](../models/syncreconcileresponseregistryurlsecretref.md) | :heavy_check_mark:                                                                                         | Reference to a Kubernetes Secret                                                                           |