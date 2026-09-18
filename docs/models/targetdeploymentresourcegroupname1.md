# TargetDeploymentResourceGroupName1

## Example Usage

```typescript
import { TargetDeploymentResourceGroupName1 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentResourceGroupName1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                    | [models.TargetDeploymentResourceGroupNameSecretRef1](../models/targetdeploymentresourcegroupnamesecretref1.md) | :heavy_check_mark:                                                                                             | Reference to a Kubernetes Secret                                                                               |