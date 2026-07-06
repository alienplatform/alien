# SyncReconcileResponseUsername1

## Example Usage

```typescript
import { SyncReconcileResponseUsername1 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseUsername1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                            | [models.SyncReconcileResponseUsernameSecretRef1](../models/syncreconcileresponseusernamesecretref1.md) | :heavy_check_mark:                                                                                     | Reference to a Kubernetes Secret                                                                       |