# SyncAcquireResponseDeploymentPort3

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPort3 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPort3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                    | [models.SyncAcquireResponseDeploymentPortSecretRef3](../models/syncacquireresponsedeploymentportsecretref3.md) | :heavy_check_mark:                                                                                             | Reference to a Kubernetes Secret                                                                               |