# SyncReconcileResponseProjectId

## Example Usage

```typescript
import { SyncReconcileResponseProjectId } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseProjectId = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                            | [models.SyncReconcileResponseProjectIdSecretRef](../models/syncreconcileresponseprojectidsecretref.md) | :heavy_check_mark:                                                                                     | Reference to a Kubernetes Secret                                                                       |