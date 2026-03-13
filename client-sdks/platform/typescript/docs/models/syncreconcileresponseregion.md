# SyncReconcileResponseRegion

## Example Usage

```typescript
import { SyncReconcileResponseRegion } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseRegion = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                      | [models.SyncReconcileResponseRegionSecretRef](../models/syncreconcileresponseregionsecretref.md) | :heavy_check_mark:                                                                               | Reference to a Kubernetes Secret                                                                 |