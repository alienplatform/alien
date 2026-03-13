# SyncReconcileResponseVaultPrefix3

## Example Usage

```typescript
import { SyncReconcileResponseVaultPrefix3 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseVaultPrefix3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                  | [models.SyncReconcileResponseVaultPrefixSecretRef3](../models/syncreconcileresponsevaultprefixsecretref3.md) | :heavy_check_mark:                                                                                           | Reference to a Kubernetes Secret                                                                             |