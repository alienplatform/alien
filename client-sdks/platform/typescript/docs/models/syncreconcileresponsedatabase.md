# SyncReconcileResponseDatabase

## Example Usage

```typescript
import { SyncReconcileResponseDatabase } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseDatabase = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                          | [models.SyncReconcileResponseDatabaseSecretRef](../models/syncreconcileresponsedatabasesecretref.md) | :heavy_check_mark:                                                                                   | Reference to a Kubernetes Secret                                                                     |