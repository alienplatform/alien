# SyncReconcileResponseDatabase5

## Example Usage

```typescript
import { SyncReconcileResponseDatabase5 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseDatabase5 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                            | [models.SyncReconcileResponseDatabaseSecretRef5](../models/syncreconcileresponsedatabasesecretref5.md) | :heavy_check_mark:                                                                                     | Reference to a Kubernetes Secret                                                                       |