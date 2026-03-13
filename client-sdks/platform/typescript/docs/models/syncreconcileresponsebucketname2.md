# SyncReconcileResponseBucketName2

## Example Usage

```typescript
import { SyncReconcileResponseBucketName2 } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseBucketName2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                | [models.SyncReconcileResponseBucketNameSecretRef2](../models/syncreconcileresponsebucketnamesecretref2.md) | :heavy_check_mark:                                                                                         | Reference to a Kubernetes Secret                                                                           |