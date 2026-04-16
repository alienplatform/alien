# SyncReconcileResponseDatabaseId

## Example Usage

```typescript
import { SyncReconcileResponseDatabaseId } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseDatabaseId = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                              | [models.SyncReconcileResponseDatabaseIdSecretRef](../models/syncreconcileresponsedatabaseidsecretref.md) | :heavy_check_mark:                                                                                       | Reference to a Kubernetes Secret                                                                         |