# SyncReconcileResponseRepositoryPrefix1

## Example Usage

```typescript
import { SyncReconcileResponseRepositoryPrefix1 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseRepositoryPrefix1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                            | [models.SyncReconcileResponseRepositoryPrefixSecretRef1](../models/syncreconcileresponserepositoryprefixsecretref1.md) | :heavy_check_mark:                                                                                                     | Reference to a Kubernetes Secret                                                                                       |