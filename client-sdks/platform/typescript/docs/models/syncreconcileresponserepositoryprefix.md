# SyncReconcileResponseRepositoryPrefix

## Example Usage

```typescript
import { SyncReconcileResponseRepositoryPrefix } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseRepositoryPrefix = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                          | [models.SyncReconcileResponseRepositoryPrefixSecretRef](../models/syncreconcileresponserepositoryprefixsecretref.md) | :heavy_check_mark:                                                                                                   | Reference to a Kubernetes Secret                                                                                     |