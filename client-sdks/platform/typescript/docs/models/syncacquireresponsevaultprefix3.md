# SyncAcquireResponseVaultPrefix3

## Example Usage

```typescript
import { SyncAcquireResponseVaultPrefix3 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseVaultPrefix3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                              | [models.SyncAcquireResponseVaultPrefixSecretRef3](../models/syncacquireresponsevaultprefixsecretref3.md) | :heavy_check_mark:                                                                                       | Reference to a Kubernetes Secret                                                                         |