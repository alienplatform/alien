# SyncReconcileResponseVaultPrefix2

## Example Usage

```typescript
import { SyncReconcileResponseVaultPrefix2 } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseVaultPrefix2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                  | [models.SyncReconcileResponseVaultPrefixSecretRef2](../models/syncreconcileresponsevaultprefixsecretref2.md) | :heavy_check_mark:                                                                                           | Reference to a Kubernetes Secret                                                                             |