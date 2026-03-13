# SyncAcquireResponseRepositoryPrefix

## Example Usage

```typescript
import { SyncAcquireResponseRepositoryPrefix } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseRepositoryPrefix = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                      | [models.SyncAcquireResponseRepositoryPrefixSecretRef](../models/syncacquireresponserepositoryprefixsecretref.md) | :heavy_check_mark:                                                                                               | Reference to a Kubernetes Secret                                                                                 |