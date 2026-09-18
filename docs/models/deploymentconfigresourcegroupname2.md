# DeploymentConfigResourceGroupName2

## Example Usage

```typescript
import { DeploymentConfigResourceGroupName2 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigResourceGroupName2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                    | [models.DeploymentConfigResourceGroupNameSecretRef2](../models/deploymentconfigresourcegroupnamesecretref2.md) | :heavy_check_mark:                                                                                             | Reference to a Kubernetes Secret                                                                               |