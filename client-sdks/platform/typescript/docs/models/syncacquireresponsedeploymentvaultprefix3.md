# SyncAcquireResponseDeploymentVaultPrefix3

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentVaultPrefix3 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentVaultPrefix3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                  | [models.SyncAcquireResponseDeploymentVaultPrefixSecretRef3](../models/syncacquireresponsedeploymentvaultprefixsecretref3.md) | :heavy_check_mark:                                                                                                           | Reference to a Kubernetes Secret                                                                                             |