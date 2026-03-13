# SyncAcquireResponseAccountName2

## Example Usage

```typescript
import { SyncAcquireResponseAccountName2 } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseAccountName2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                              | [models.SyncAcquireResponseAccountNameSecretRef2](../models/syncacquireresponseaccountnamesecretref2.md) | :heavy_check_mark:                                                                                       | Reference to a Kubernetes Secret                                                                         |