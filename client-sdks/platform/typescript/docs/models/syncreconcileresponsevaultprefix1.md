# SyncReconcileResponseVaultPrefix1

## Example Usage

```typescript
import { SyncReconcileResponseVaultPrefix1 } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseVaultPrefix1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                  | [models.SyncReconcileResponseVaultPrefixSecretRef1](../models/syncreconcileresponsevaultprefixsecretref1.md) | :heavy_check_mark:                                                                                           | Reference to a Kubernetes Secret                                                                             |