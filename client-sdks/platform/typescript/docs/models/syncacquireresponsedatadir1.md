# SyncAcquireResponseDataDir1

## Example Usage

```typescript
import { SyncAcquireResponseDataDir1 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDataDir1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                      | [models.SyncAcquireResponseDataDirSecretRef1](../models/syncacquireresponsedatadirsecretref1.md) | :heavy_check_mark:                                                                               | Reference to a Kubernetes Secret                                                                 |