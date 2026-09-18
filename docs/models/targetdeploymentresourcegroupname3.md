# TargetDeploymentResourceGroupName3

## Example Usage

```typescript
import { TargetDeploymentResourceGroupName3 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentResourceGroupName3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                    | [models.TargetDeploymentResourceGroupNameSecretRef3](../models/targetdeploymentresourcegroupnamesecretref3.md) | :heavy_check_mark:                                                                                             | Reference to a Kubernetes Secret                                                                               |