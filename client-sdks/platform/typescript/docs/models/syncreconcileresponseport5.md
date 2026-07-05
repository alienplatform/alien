# SyncReconcileResponsePort5

## Example Usage

```typescript
import { SyncReconcileResponsePort5 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePort5 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                    | [models.SyncReconcileResponsePortSecretRef5](../models/syncreconcileresponseportsecretref5.md) | :heavy_check_mark:                                                                             | Reference to a Kubernetes Secret                                                               |