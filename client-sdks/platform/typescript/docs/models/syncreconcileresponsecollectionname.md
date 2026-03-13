# SyncReconcileResponseCollectionName

## Example Usage

```typescript
import { SyncReconcileResponseCollectionName } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseCollectionName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                      | [models.SyncReconcileResponseCollectionNameSecretRef](../models/syncreconcileresponsecollectionnamesecretref.md) | :heavy_check_mark:                                                                                               | Reference to a Kubernetes Secret                                                                                 |