# SyncAcquireResponseDatabase3

## Example Usage

```typescript
import { SyncAcquireResponseDatabase3 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDatabase3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.SyncAcquireResponseDatabaseSecretRef3](../models/syncacquireresponsedatabasesecretref3.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |