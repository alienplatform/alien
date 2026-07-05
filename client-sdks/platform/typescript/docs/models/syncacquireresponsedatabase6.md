# SyncAcquireResponseDatabase6

## Example Usage

```typescript
import { SyncAcquireResponseDatabase6 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDatabase6 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.SyncAcquireResponseDatabaseSecretRef6](../models/syncacquireresponsedatabasesecretref6.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |