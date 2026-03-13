# SyncAcquireResponseVaultName

## Example Usage

```typescript
import { SyncAcquireResponseVaultName } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseVaultName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.SyncAcquireResponseVaultNameSecretRef](../models/syncacquireresponsevaultnamesecretref.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |