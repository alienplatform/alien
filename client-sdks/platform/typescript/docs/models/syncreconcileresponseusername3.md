# SyncReconcileResponseUsername3

## Example Usage

```typescript
import { SyncReconcileResponseUsername3 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseUsername3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                            | [models.SyncReconcileResponseUsernameSecretRef3](../models/syncreconcileresponseusernamesecretref3.md) | :heavy_check_mark:                                                                                     | Reference to a Kubernetes Secret                                                                       |