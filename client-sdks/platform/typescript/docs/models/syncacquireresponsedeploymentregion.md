# SyncAcquireResponseDeploymentRegion

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentRegion } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentRegion = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                      | [models.SyncAcquireResponseDeploymentRegionSecretRef](../models/syncacquireresponsedeploymentregionsecretref.md) | :heavy_check_mark:                                                                                               | Reference to a Kubernetes Secret                                                                                 |