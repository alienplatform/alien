# SyncReconcileResponseDatabase4

## Example Usage

```typescript
import { SyncReconcileResponseDatabase4 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseDatabase4 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                            | [models.SyncReconcileResponseDatabaseSecretRef4](../models/syncreconcileresponsedatabasesecretref4.md) | :heavy_check_mark:                                                                                     | Reference to a Kubernetes Secret                                                                       |