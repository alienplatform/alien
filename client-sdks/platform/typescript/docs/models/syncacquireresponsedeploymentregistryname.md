# SyncAcquireResponseDeploymentRegistryName

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentRegistryName } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentRegistryName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                  | [models.SyncAcquireResponseDeploymentRegistryNameSecretRef](../models/syncacquireresponsedeploymentregistrynamesecretref.md) | :heavy_check_mark:                                                                                                           | Reference to a Kubernetes Secret                                                                                             |