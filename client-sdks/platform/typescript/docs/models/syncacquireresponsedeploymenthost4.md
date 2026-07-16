# SyncAcquireResponseDeploymentHost4

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentHost4 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentHost4 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                    | [models.SyncAcquireResponseDeploymentHostSecretRef4](../models/syncacquireresponsedeploymenthostsecretref4.md) | :heavy_check_mark:                                                                                             | Reference to a Kubernetes Secret                                                                               |