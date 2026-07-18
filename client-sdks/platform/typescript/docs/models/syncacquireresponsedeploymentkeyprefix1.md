# SyncAcquireResponseDeploymentKeyPrefix1

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentKeyPrefix1 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentKeyPrefix1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                              | [models.SyncAcquireResponseDeploymentKeyPrefixSecretRef1](../models/syncacquireresponsedeploymentkeyprefixsecretref1.md) | :heavy_check_mark:                                                                                                       | Reference to a Kubernetes Secret                                                                                         |