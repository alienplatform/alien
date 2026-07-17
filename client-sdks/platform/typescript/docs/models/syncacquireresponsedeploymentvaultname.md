# SyncAcquireResponseDeploymentVaultName

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentVaultName } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentVaultName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                            | [models.SyncAcquireResponseDeploymentVaultNameSecretRef](../models/syncacquireresponsedeploymentvaultnamesecretref.md) | :heavy_check_mark:                                                                                                     | Reference to a Kubernetes Secret                                                                                       |