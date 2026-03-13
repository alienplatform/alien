# SyncReconcileResponseStoragePath

## Example Usage

```typescript
import { SyncReconcileResponseStoragePath } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseStoragePath = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                | [models.SyncReconcileResponseStoragePathSecretRef](../models/syncreconcileresponsestoragepathsecretref.md) | :heavy_check_mark:                                                                                         | Reference to a Kubernetes Secret                                                                           |