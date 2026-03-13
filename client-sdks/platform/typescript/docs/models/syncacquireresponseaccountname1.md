# SyncAcquireResponseAccountName1

## Example Usage

```typescript
import { SyncAcquireResponseAccountName1 } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseAccountName1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                              | [models.SyncAcquireResponseAccountNameSecretRef1](../models/syncacquireresponseaccountnamesecretref1.md) | :heavy_check_mark:                                                                                       | Reference to a Kubernetes Secret                                                                         |