# SyncReconcileResponseHost3

## Example Usage

```typescript
import { SyncReconcileResponseHost3 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseHost3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                    | [models.SyncReconcileResponseHostSecretRef3](../models/syncreconcileresponsehostsecretref3.md) | :heavy_check_mark:                                                                             | Reference to a Kubernetes Secret                                                               |