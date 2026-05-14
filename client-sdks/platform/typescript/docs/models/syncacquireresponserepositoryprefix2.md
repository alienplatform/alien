# SyncAcquireResponseRepositoryPrefix2

## Example Usage

```typescript
import { SyncAcquireResponseRepositoryPrefix2 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseRepositoryPrefix2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                        | [models.SyncAcquireResponseRepositoryPrefixSecretRef2](../models/syncacquireresponserepositoryprefixsecretref2.md) | :heavy_check_mark:                                                                                                 | Reference to a Kubernetes Secret                                                                                   |