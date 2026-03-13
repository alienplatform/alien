# SyncReconcileResponseDataDir3

## Example Usage

```typescript
import { SyncReconcileResponseDataDir3 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseDataDir3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                          | [models.SyncReconcileResponseDataDirSecretRef3](../models/syncreconcileresponsedatadirsecretref3.md) | :heavy_check_mark:                                                                                   | Reference to a Kubernetes Secret                                                                     |