# SyncReconcileResponseRepositoryName

## Example Usage

```typescript
import { SyncReconcileResponseRepositoryName } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseRepositoryName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                      | [models.SyncReconcileResponseRepositoryNameSecretRef](../models/syncreconcileresponserepositorynamesecretref.md) | :heavy_check_mark:                                                                                               | Reference to a Kubernetes Secret                                                                                 |