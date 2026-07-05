# SyncReconcileResponseDatabase2

## Example Usage

```typescript
import { SyncReconcileResponseDatabase2 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseDatabase2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                            | [models.SyncReconcileResponseDatabaseSecretRef2](../models/syncreconcileresponsedatabasesecretref2.md) | :heavy_check_mark:                                                                                     | Reference to a Kubernetes Secret                                                                       |