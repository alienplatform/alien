# SyncAcquireResponseDeploymentHost1

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentHost1 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentHost1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                    | [models.SyncAcquireResponseDeploymentHostSecretRef1](../models/syncacquireresponsedeploymenthostsecretref1.md) | :heavy_check_mark:                                                                                             | Reference to a Kubernetes Secret                                                                               |