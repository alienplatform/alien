# SyncReconcileResponseRepositoryPrefix2

## Example Usage

```typescript
import { SyncReconcileResponseRepositoryPrefix2 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseRepositoryPrefix2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                            | [models.SyncReconcileResponseRepositoryPrefixSecretRef2](../models/syncreconcileresponserepositoryprefixsecretref2.md) | :heavy_check_mark:                                                                                                     | Reference to a Kubernetes Secret                                                                                       |