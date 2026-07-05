# SyncReconcileResponseDatabase3

## Example Usage

```typescript
import { SyncReconcileResponseDatabase3 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseDatabase3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                            | [models.SyncReconcileResponseDatabaseSecretRef3](../models/syncreconcileresponsedatabasesecretref3.md) | :heavy_check_mark:                                                                                     | Reference to a Kubernetes Secret                                                                       |