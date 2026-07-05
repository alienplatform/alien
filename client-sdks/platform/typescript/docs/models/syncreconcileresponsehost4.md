# SyncReconcileResponseHost4

## Example Usage

```typescript
import { SyncReconcileResponseHost4 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseHost4 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                    | [models.SyncReconcileResponseHostSecretRef4](../models/syncreconcileresponsehostsecretref4.md) | :heavy_check_mark:                                                                             | Reference to a Kubernetes Secret                                                               |