# DeploymentConfigResourceGroupName1

## Example Usage

```typescript
import { DeploymentConfigResourceGroupName1 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigResourceGroupName1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                    | [models.DeploymentConfigResourceGroupNameSecretRef1](../models/deploymentconfigresourcegroupnamesecretref1.md) | :heavy_check_mark:                                                                                             | Reference to a Kubernetes Secret                                                                               |