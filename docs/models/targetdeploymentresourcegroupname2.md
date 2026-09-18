# TargetDeploymentResourceGroupName2

## Example Usage

```typescript
import { TargetDeploymentResourceGroupName2 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentResourceGroupName2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                    | [models.TargetDeploymentResourceGroupNameSecretRef2](../models/targetdeploymentresourcegroupnamesecretref2.md) | :heavy_check_mark:                                                                                             | Reference to a Kubernetes Secret                                                                               |