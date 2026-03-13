# SyncReconcileResponseResourceGroupName1

## Example Usage

```typescript
import { SyncReconcileResponseResourceGroupName1 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseResourceGroupName1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                              | [models.SyncReconcileResponseResourceGroupNameSecretRef1](../models/syncreconcileresponseresourcegroupnamesecretref1.md) | :heavy_check_mark:                                                                                                       | Reference to a Kubernetes Secret                                                                                         |