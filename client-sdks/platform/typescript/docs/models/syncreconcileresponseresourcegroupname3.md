# SyncReconcileResponseResourceGroupName3

## Example Usage

```typescript
import { SyncReconcileResponseResourceGroupName3 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseResourceGroupName3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                              | [models.SyncReconcileResponseResourceGroupNameSecretRef3](../models/syncreconcileresponseresourcegroupnamesecretref3.md) | :heavy_check_mark:                                                                                                       | Reference to a Kubernetes Secret                                                                                         |