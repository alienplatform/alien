# SyncReconcileResponseTableName1

## Example Usage

```typescript
import { SyncReconcileResponseTableName1 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseTableName1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                              | [models.SyncReconcileResponseTableNameSecretRef1](../models/syncreconcileresponsetablenamesecretref1.md) | :heavy_check_mark:                                                                                       | Reference to a Kubernetes Secret                                                                         |