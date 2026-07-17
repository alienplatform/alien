# SyncAcquireResponseDeploymentHost2

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentHost2 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentHost2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                    | [models.SyncAcquireResponseDeploymentHostSecretRef2](../models/syncacquireresponsedeploymenthostsecretref2.md) | :heavy_check_mark:                                                                                             | Reference to a Kubernetes Secret                                                                               |