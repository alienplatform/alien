# SyncAcquireResponseDeploymentAccountName2

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentAccountName2 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentAccountName2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                  | [models.SyncAcquireResponseDeploymentAccountNameSecretRef2](../models/syncacquireresponsedeploymentaccountnamesecretref2.md) | :heavy_check_mark:                                                                                                           | Reference to a Kubernetes Secret                                                                                             |