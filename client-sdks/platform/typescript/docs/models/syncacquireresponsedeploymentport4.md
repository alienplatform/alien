# SyncAcquireResponseDeploymentPort4

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPort4 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPort4 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                    | [models.SyncAcquireResponseDeploymentPortSecretRef4](../models/syncacquireresponsedeploymentportsecretref4.md) | :heavy_check_mark:                                                                                             | Reference to a Kubernetes Secret                                                                               |