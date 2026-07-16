# SyncAcquireResponseDeploymentTableName2

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentTableName2 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentTableName2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                              | [models.SyncAcquireResponseDeploymentTableNameSecretRef2](../models/syncacquireresponsedeploymenttablenamesecretref2.md) | :heavy_check_mark:                                                                                                       | Reference to a Kubernetes Secret                                                                                         |