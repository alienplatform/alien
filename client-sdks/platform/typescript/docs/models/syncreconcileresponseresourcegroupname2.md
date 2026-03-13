# SyncReconcileResponseResourceGroupName2

## Example Usage

```typescript
import { SyncReconcileResponseResourceGroupName2 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseResourceGroupName2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                              | [models.SyncReconcileResponseResourceGroupNameSecretRef2](../models/syncreconcileresponseresourcegroupnamesecretref2.md) | :heavy_check_mark:                                                                                                       | Reference to a Kubernetes Secret                                                                                         |