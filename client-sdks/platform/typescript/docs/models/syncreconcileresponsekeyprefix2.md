# SyncReconcileResponseKeyPrefix2

## Example Usage

```typescript
import { SyncReconcileResponseKeyPrefix2 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseKeyPrefix2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                              | [models.SyncReconcileResponseKeyPrefixSecretRef2](../models/syncreconcileresponsekeyprefixsecretref2.md) | :heavy_check_mark:                                                                                       | Reference to a Kubernetes Secret                                                                         |