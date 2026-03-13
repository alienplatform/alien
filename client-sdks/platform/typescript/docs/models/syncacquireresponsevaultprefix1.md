# SyncAcquireResponseVaultPrefix1

## Example Usage

```typescript
import { SyncAcquireResponseVaultPrefix1 } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseVaultPrefix1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                              | [models.SyncAcquireResponseVaultPrefixSecretRef1](../models/syncacquireresponsevaultprefixsecretref1.md) | :heavy_check_mark:                                                                                       | Reference to a Kubernetes Secret                                                                         |