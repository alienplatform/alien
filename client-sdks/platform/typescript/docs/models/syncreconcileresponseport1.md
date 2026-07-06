# SyncReconcileResponsePort1

## Example Usage

```typescript
import { SyncReconcileResponsePort1 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePort1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                    | [models.SyncReconcileResponsePortSecretRef1](../models/syncreconcileresponseportsecretref1.md) | :heavy_check_mark:                                                                             | Reference to a Kubernetes Secret                                                               |