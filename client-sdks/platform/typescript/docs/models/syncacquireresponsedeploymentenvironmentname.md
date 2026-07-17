# SyncAcquireResponseDeploymentEnvironmentName

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentEnvironmentName } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentEnvironmentName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                        | [models.SyncAcquireResponseDeploymentEnvironmentNameSecretRef](../models/syncacquireresponsedeploymentenvironmentnamesecretref.md) | :heavy_check_mark:                                                                                                                 | Reference to a Kubernetes Secret                                                                                                   |