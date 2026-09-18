# TargetDeploymentPullRoleArn

## Example Usage

```typescript
import { TargetDeploymentPullRoleArn } from "@alienplatform/platform-api/models";

let value: TargetDeploymentPullRoleArn = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                      | [models.TargetDeploymentPullRoleArnSecretRef](../models/targetdeploymentpullrolearnsecretref.md) | :heavy_check_mark:                                                                               | Reference to a Kubernetes Secret                                                                 |