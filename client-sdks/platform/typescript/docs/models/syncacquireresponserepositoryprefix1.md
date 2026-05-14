# SyncAcquireResponseRepositoryPrefix1

## Example Usage

```typescript
import { SyncAcquireResponseRepositoryPrefix1 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseRepositoryPrefix1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                        | [models.SyncAcquireResponseRepositoryPrefixSecretRef1](../models/syncacquireresponserepositoryprefixsecretref1.md) | :heavy_check_mark:                                                                                                 | Reference to a Kubernetes Secret                                                                                   |