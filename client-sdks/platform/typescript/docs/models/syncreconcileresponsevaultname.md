# SyncReconcileResponseVaultName

## Example Usage

```typescript
import { SyncReconcileResponseVaultName } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseVaultName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                            | [models.SyncReconcileResponseVaultNameSecretRef](../models/syncreconcileresponsevaultnamesecretref.md) | :heavy_check_mark:                                                                                     | Reference to a Kubernetes Secret                                                                       |