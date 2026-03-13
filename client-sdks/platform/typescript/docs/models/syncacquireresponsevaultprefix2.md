# SyncAcquireResponseVaultPrefix2

## Example Usage

```typescript
import { SyncAcquireResponseVaultPrefix2 } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseVaultPrefix2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                              | [models.SyncAcquireResponseVaultPrefixSecretRef2](../models/syncacquireresponsevaultprefixsecretref2.md) | :heavy_check_mark:                                                                                       | Reference to a Kubernetes Secret                                                                         |