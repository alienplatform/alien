# SyncReconcileResponseHost1

## Example Usage

```typescript
import { SyncReconcileResponseHost1 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseHost1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                    | [models.SyncReconcileResponseHostSecretRef1](../models/syncreconcileresponsehostsecretref1.md) | :heavy_check_mark:                                                                             | Reference to a Kubernetes Secret                                                               |