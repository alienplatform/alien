# SyncAcquireResponseDataDir2

## Example Usage

```typescript
import { SyncAcquireResponseDataDir2 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDataDir2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                      | [models.SyncAcquireResponseDataDirSecretRef2](../models/syncacquireresponsedatadirsecretref2.md) | :heavy_check_mark:                                                                               | Reference to a Kubernetes Secret                                                                 |