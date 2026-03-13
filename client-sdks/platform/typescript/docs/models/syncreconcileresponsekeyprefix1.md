# SyncReconcileResponseKeyPrefix1

## Example Usage

```typescript
import { SyncReconcileResponseKeyPrefix1 } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseKeyPrefix1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                              | [models.SyncReconcileResponseKeyPrefixSecretRef1](../models/syncreconcileresponsekeyprefixsecretref1.md) | :heavy_check_mark:                                                                                       | Reference to a Kubernetes Secret                                                                         |