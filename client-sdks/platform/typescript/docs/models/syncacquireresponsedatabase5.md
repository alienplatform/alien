# SyncAcquireResponseDatabase5

## Example Usage

```typescript
import { SyncAcquireResponseDatabase5 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDatabase5 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                        | [models.SyncAcquireResponseDatabaseSecretRef5](../models/syncacquireresponsedatabasesecretref5.md) | :heavy_check_mark:                                                                                 | Reference to a Kubernetes Secret                                                                   |