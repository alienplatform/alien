# DeploymentConfigResourceGroupName3

## Example Usage

```typescript
import { DeploymentConfigResourceGroupName3 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigResourceGroupName3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                    | [models.DeploymentConfigResourceGroupNameSecretRef3](../models/deploymentconfigresourcegroupnamesecretref3.md) | :heavy_check_mark:                                                                                             | Reference to a Kubernetes Secret                                                                               |