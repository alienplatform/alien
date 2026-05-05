# SyncReconcileResponseResourceId

## Example Usage

```typescript
import { SyncReconcileResponseResourceId } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseResourceId = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                              | [models.SyncReconcileResponseResourceIdSecretRef](../models/syncreconcileresponseresourceidsecretref.md) | :heavy_check_mark:                                                                                       | Reference to a Kubernetes Secret                                                                         |