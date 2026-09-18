# DeploymentConfigPushRoleArn

## Example Usage

```typescript
import { DeploymentConfigPushRoleArn } from "@alienplatform/platform-api/models";

let value: DeploymentConfigPushRoleArn = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                      | [models.DeploymentConfigPushRoleArnSecretRef](../models/deploymentconfigpushrolearnsecretref.md) | :heavy_check_mark:                                                                               | Reference to a Kubernetes Secret                                                                 |