# SyncReconcileResponsePasswordSecretName

## Example Usage

```typescript
import { SyncReconcileResponsePasswordSecretName } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePasswordSecretName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                              | [models.SyncReconcileResponsePasswordSecretNameSecretRef](../models/syncreconcileresponsepasswordsecretnamesecretref.md) | :heavy_check_mark:                                                                                                       | Reference to a Kubernetes Secret                                                                                         |