# DeploymentConfigPullRoleArn

## Example Usage

```typescript
import { DeploymentConfigPullRoleArn } from "@alienplatform/platform-api/models";

let value: DeploymentConfigPullRoleArn = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                      | [models.DeploymentConfigPullRoleArnSecretRef](../models/deploymentconfigpullrolearnsecretref.md) | :heavy_check_mark:                                                                               | Reference to a Kubernetes Secret                                                                 |