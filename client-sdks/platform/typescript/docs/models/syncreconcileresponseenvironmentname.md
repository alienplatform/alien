# SyncReconcileResponseEnvironmentName

## Example Usage

```typescript
import { SyncReconcileResponseEnvironmentName } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseEnvironmentName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                        | [models.SyncReconcileResponseEnvironmentNameSecretRef](../models/syncreconcileresponseenvironmentnamesecretref.md) | :heavy_check_mark:                                                                                                 | Reference to a Kubernetes Secret                                                                                   |