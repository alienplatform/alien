# SyncAcquireResponseDeploymentHost3

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentHost3 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentHost3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                    | [models.SyncAcquireResponseDeploymentHostSecretRef3](../models/syncacquireresponsedeploymenthostsecretref3.md) | :heavy_check_mark:                                                                                             | Reference to a Kubernetes Secret                                                                               |