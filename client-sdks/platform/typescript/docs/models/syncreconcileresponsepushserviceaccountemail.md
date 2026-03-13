# SyncReconcileResponsePushServiceAccountEmail

## Example Usage

```typescript
import { SyncReconcileResponsePushServiceAccountEmail } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponsePushServiceAccountEmail = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                        | [models.SyncReconcileResponsePushServiceAccountEmailSecretRef](../models/syncreconcileresponsepushserviceaccountemailsecretref.md) | :heavy_check_mark:                                                                                                                 | Reference to a Kubernetes Secret                                                                                                   |