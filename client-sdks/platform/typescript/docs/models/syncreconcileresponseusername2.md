# SyncReconcileResponseUsername2

## Example Usage

```typescript
import { SyncReconcileResponseUsername2 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseUsername2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                            | [models.SyncReconcileResponseUsernameSecretRef2](../models/syncreconcileresponseusernamesecretref2.md) | :heavy_check_mark:                                                                                     | Reference to a Kubernetes Secret                                                                       |