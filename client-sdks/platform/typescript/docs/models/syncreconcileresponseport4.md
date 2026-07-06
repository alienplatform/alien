# SyncReconcileResponsePort4

## Example Usage

```typescript
import { SyncReconcileResponsePort4 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePort4 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                    | [models.SyncReconcileResponsePortSecretRef4](../models/syncreconcileresponseportsecretref4.md) | :heavy_check_mark:                                                                             | Reference to a Kubernetes Secret                                                               |