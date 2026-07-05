# SyncReconcileResponseUsername4

## Example Usage

```typescript
import { SyncReconcileResponseUsername4 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseUsername4 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                            | [models.SyncReconcileResponseUsernameSecretRef4](../models/syncreconcileresponseusernamesecretref4.md) | :heavy_check_mark:                                                                                     | Reference to a Kubernetes Secret                                                                       |