# SyncAcquireResponseDeploymentPort1

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPort1 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPort1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                    | [models.SyncAcquireResponseDeploymentPortSecretRef1](../models/syncacquireresponsedeploymentportsecretref1.md) | :heavy_check_mark:                                                                                             | Reference to a Kubernetes Secret                                                                               |