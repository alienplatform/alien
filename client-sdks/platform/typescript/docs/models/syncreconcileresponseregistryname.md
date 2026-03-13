# SyncReconcileResponseRegistryName

## Example Usage

```typescript
import { SyncReconcileResponseRegistryName } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseRegistryName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                  | [models.SyncReconcileResponseRegistryNameSecretRef](../models/syncreconcileresponseregistrynamesecretref.md) | :heavy_check_mark:                                                                                           | Reference to a Kubernetes Secret                                                                             |