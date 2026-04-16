# SyncReconcileResponseNamespace2

## Example Usage

```typescript
import { SyncReconcileResponseNamespace2 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseNamespace2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                              | [models.SyncReconcileResponseNamespaceSecretRef2](../models/syncreconcileresponsenamespacesecretref2.md) | :heavy_check_mark:                                                                                       | Reference to a Kubernetes Secret                                                                         |