# SyncAcquireResponseDeploymentTableName1

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentTableName1 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentTableName1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                              | [models.SyncAcquireResponseDeploymentTableNameSecretRef1](../models/syncacquireresponsedeploymenttablenamesecretref1.md) | :heavy_check_mark:                                                                                                       | Reference to a Kubernetes Secret                                                                                         |