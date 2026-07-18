# SyncAcquireResponseDeploymentKeyPrefix2

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentKeyPrefix2 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentKeyPrefix2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                              | [models.SyncAcquireResponseDeploymentKeyPrefixSecretRef2](../models/syncacquireresponsedeploymentkeyprefixsecretref2.md) | :heavy_check_mark:                                                                                                       | Reference to a Kubernetes Secret                                                                                         |