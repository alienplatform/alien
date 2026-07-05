# SyncReconcileResponsePort3

## Example Usage

```typescript
import { SyncReconcileResponsePort3 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePort3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                    | [models.SyncReconcileResponsePortSecretRef3](../models/syncreconcileresponseportsecretref3.md) | :heavy_check_mark:                                                                             | Reference to a Kubernetes Secret                                                               |