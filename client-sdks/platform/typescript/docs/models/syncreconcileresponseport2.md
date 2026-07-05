# SyncReconcileResponsePort2

## Example Usage

```typescript
import { SyncReconcileResponsePort2 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePort2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                    | [models.SyncReconcileResponsePortSecretRef2](../models/syncreconcileresponseportsecretref2.md) | :heavy_check_mark:                                                                             | Reference to a Kubernetes Secret                                                               |