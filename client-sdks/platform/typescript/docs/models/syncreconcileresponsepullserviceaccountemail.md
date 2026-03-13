# SyncReconcileResponsePullServiceAccountEmail

## Example Usage

```typescript
import { SyncReconcileResponsePullServiceAccountEmail } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponsePullServiceAccountEmail = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                        | [models.SyncReconcileResponsePullServiceAccountEmailSecretRef](../models/syncreconcileresponsepullserviceaccountemailsecretref.md) | :heavy_check_mark:                                                                                                                 | Reference to a Kubernetes Secret                                                                                                   |