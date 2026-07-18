# SyncAcquireResponseDeploymentAccountName1

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentAccountName1 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentAccountName1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                  | [models.SyncAcquireResponseDeploymentAccountNameSecretRef1](../models/syncacquireresponsedeploymentaccountnamesecretref1.md) | :heavy_check_mark:                                                                                                           | Reference to a Kubernetes Secret                                                                                             |