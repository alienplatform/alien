# SyncReconcileResponseDataDir2

## Example Usage

```typescript
import { SyncReconcileResponseDataDir2 } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseDataDir2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                          | [models.SyncReconcileResponseDataDirSecretRef2](../models/syncreconcileresponsedatadirsecretref2.md) | :heavy_check_mark:                                                                                   | Reference to a Kubernetes Secret                                                                     |