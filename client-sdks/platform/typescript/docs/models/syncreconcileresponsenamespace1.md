# SyncReconcileResponseNamespace1

## Example Usage

```typescript
import { SyncReconcileResponseNamespace1 } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseNamespace1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                              | [models.SyncReconcileResponseNamespaceSecretRef1](../models/syncreconcileresponsenamespacesecretref1.md) | :heavy_check_mark:                                                                                       | Reference to a Kubernetes Secret                                                                         |