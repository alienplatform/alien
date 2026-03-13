# SyncReconcileResponseTableName2

## Example Usage

```typescript
import { SyncReconcileResponseTableName2 } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseTableName2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                              | [models.SyncReconcileResponseTableNameSecretRef2](../models/syncreconcileresponsetablenamesecretref2.md) | :heavy_check_mark:                                                                                       | Reference to a Kubernetes Secret                                                                         |