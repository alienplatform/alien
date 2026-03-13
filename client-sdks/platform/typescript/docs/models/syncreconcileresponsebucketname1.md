# SyncReconcileResponseBucketName1

## Example Usage

```typescript
import { SyncReconcileResponseBucketName1 } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseBucketName1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                | [models.SyncReconcileResponseBucketNameSecretRef1](../models/syncreconcileresponsebucketnamesecretref1.md) | :heavy_check_mark:                                                                                         | Reference to a Kubernetes Secret                                                                           |