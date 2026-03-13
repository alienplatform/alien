# SyncAcquireResponseDataDir3

## Example Usage

```typescript
import { SyncAcquireResponseDataDir3 } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseDataDir3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                      | [models.SyncAcquireResponseDataDirSecretRef3](../models/syncacquireresponsedatadirsecretref3.md) | :heavy_check_mark:                                                                               | Reference to a Kubernetes Secret                                                                 |