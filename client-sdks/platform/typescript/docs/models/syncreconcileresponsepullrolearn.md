# SyncReconcileResponsePullRoleArn

## Example Usage

```typescript
import { SyncReconcileResponsePullRoleArn } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponsePullRoleArn = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                | [models.SyncReconcileResponsePullRoleArnSecretRef](../models/syncreconcileresponsepullrolearnsecretref.md) | :heavy_check_mark:                                                                                         | Reference to a Kubernetes Secret                                                                           |