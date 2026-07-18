# SyncAcquireResponseDeploymentPort2

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPort2 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPort2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                    | [models.SyncAcquireResponseDeploymentPortSecretRef2](../models/syncacquireresponsedeploymentportsecretref2.md) | :heavy_check_mark:                                                                                             | Reference to a Kubernetes Secret                                                                               |