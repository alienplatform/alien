# SyncReconcileResponseDataDir1

## Example Usage

```typescript
import { SyncReconcileResponseDataDir1 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseDataDir1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                          | [models.SyncReconcileResponseDataDirSecretRef1](../models/syncreconcileresponsedatadirsecretref1.md) | :heavy_check_mark:                                                                                   | Reference to a Kubernetes Secret                                                                     |