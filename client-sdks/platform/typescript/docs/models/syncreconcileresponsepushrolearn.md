# SyncReconcileResponsePushRoleArn

## Example Usage

```typescript
import { SyncReconcileResponsePushRoleArn } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponsePushRoleArn = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                | [models.SyncReconcileResponsePushRoleArnSecretRef](../models/syncreconcileresponsepushrolearnsecretref.md) | :heavy_check_mark:                                                                                         | Reference to a Kubernetes Secret                                                                           |