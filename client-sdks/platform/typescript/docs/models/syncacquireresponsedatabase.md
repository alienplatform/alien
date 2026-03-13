# SyncAcquireResponseDatabase

## Example Usage

```typescript
import { SyncAcquireResponseDatabase } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseDatabase = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                      | [models.SyncAcquireResponseDatabaseSecretRef](../models/syncacquireresponsedatabasesecretref.md) | :heavy_check_mark:                                                                               | Reference to a Kubernetes Secret                                                                 |