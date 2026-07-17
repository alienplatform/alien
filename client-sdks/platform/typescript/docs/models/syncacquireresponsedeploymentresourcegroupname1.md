# SyncAcquireResponseDeploymentResourceGroupName1

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentResourceGroupName1 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentResourceGroupName1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                                    | Type                                                                                                                                     | Required                                                                                                                                 | Description                                                                                                                              |
| ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                                              | [models.SyncAcquireResponseDeploymentResourceGroupNameSecretRef1](../models/syncacquireresponsedeploymentresourcegroupnamesecretref1.md) | :heavy_check_mark:                                                                                                                       | Reference to a Kubernetes Secret                                                                                                         |