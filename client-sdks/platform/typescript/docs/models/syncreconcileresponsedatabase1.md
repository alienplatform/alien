# SyncReconcileResponseDatabase1

## Example Usage

```typescript
import { SyncReconcileResponseDatabase1 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseDatabase1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                            | [models.SyncReconcileResponseDatabaseSecretRef1](../models/syncreconcileresponsedatabasesecretref1.md) | :heavy_check_mark:                                                                                     | Reference to a Kubernetes Secret                                                                       |