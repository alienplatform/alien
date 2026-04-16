# SyncReconcileResponseAccountName2

## Example Usage

```typescript
import { SyncReconcileResponseAccountName2 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseAccountName2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                  | [models.SyncReconcileResponseAccountNameSecretRef2](../models/syncreconcileresponseaccountnamesecretref2.md) | :heavy_check_mark:                                                                                           | Reference to a Kubernetes Secret                                                                             |