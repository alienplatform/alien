# SyncReconcileResponseHost2

## Example Usage

```typescript
import { SyncReconcileResponseHost2 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseHost2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                    | [models.SyncReconcileResponseHostSecretRef2](../models/syncreconcileresponsehostsecretref2.md) | :heavy_check_mark:                                                                             | Reference to a Kubernetes Secret                                                               |