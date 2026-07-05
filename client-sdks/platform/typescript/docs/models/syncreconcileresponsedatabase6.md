# SyncReconcileResponseDatabase6

## Example Usage

```typescript
import { SyncReconcileResponseDatabase6 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseDatabase6 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                            | [models.SyncReconcileResponseDatabaseSecretRef6](../models/syncreconcileresponsedatabasesecretref6.md) | :heavy_check_mark:                                                                                     | Reference to a Kubernetes Secret                                                                       |