# SyncAcquireResponseDeploymentVaultPrefix1

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentVaultPrefix1 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentVaultPrefix1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                  | [models.SyncAcquireResponseDeploymentVaultPrefixSecretRef1](../models/syncacquireresponsedeploymentvaultprefixsecretref1.md) | :heavy_check_mark:                                                                                                           | Reference to a Kubernetes Secret                                                                                             |