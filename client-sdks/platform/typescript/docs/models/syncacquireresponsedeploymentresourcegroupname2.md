# SyncAcquireResponseDeploymentResourceGroupName2

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentResourceGroupName2 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentResourceGroupName2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                                    | Type                                                                                                                                     | Required                                                                                                                                 | Description                                                                                                                              |
| ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                              | [models.SyncAcquireResponseDeploymentResourceGroupNameSecretRef2](../models/syncacquireresponsedeploymentresourcegroupnamesecretref2.md) | :heavy_check_mark:                                                                                                                       | Reference to a Kubernetes Secret                                                                                                         |