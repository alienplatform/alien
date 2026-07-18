# SyncAcquireResponseDeploymentVaultPrefix2

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentVaultPrefix2 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentVaultPrefix2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                  | [models.SyncAcquireResponseDeploymentVaultPrefixSecretRef2](../models/syncacquireresponsedeploymentvaultprefixsecretref2.md) | :heavy_check_mark:                                                                                                           | Reference to a Kubernetes Secret                                                                                             |