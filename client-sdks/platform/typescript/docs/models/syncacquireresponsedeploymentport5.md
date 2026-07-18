# SyncAcquireResponseDeploymentPort5

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPort5 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPort5 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                    | [models.SyncAcquireResponseDeploymentPortSecretRef5](../models/syncacquireresponsedeploymentportsecretref5.md) | :heavy_check_mark:                                                                                             | Reference to a Kubernetes Secret                                                                               |