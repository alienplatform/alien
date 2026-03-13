# SyncReconcileResponseAccountName1

## Example Usage

```typescript
import { SyncReconcileResponseAccountName1 } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseAccountName1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                  | [models.SyncReconcileResponseAccountNameSecretRef1](../models/syncreconcileresponseaccountnamesecretref1.md) | :heavy_check_mark:                                                                                           | Reference to a Kubernetes Secret                                                                             |