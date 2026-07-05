# SyncAcquireResponseDatabase4

## Example Usage

```typescript
import { SyncAcquireResponseDatabase4 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDatabase4 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.SyncAcquireResponseDatabaseSecretRef4](../models/syncacquireresponsedatabasesecretref4.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |