# SyncAcquireResponseDeploymentResourceGroupName3

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentResourceGroupName3 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentResourceGroupName3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                                    | Type                                                                                                                                     | Required                                                                                                                                 | Description                                                                                                                              |
| ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                              | [models.SyncAcquireResponseDeploymentResourceGroupNameSecretRef3](../models/syncacquireresponsedeploymentresourcegroupnamesecretref3.md) | :heavy_check_mark:                                                                                                                       | Reference to a Kubernetes Secret                                                                                                         |