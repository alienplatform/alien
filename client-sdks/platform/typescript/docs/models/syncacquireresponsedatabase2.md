# SyncAcquireResponseDatabase2

## Example Usage

```typescript
import { SyncAcquireResponseDatabase2 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDatabase2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.SyncAcquireResponseDatabaseSecretRef2](../models/syncacquireresponsedatabasesecretref2.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |