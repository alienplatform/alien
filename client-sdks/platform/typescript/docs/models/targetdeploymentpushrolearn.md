# TargetDeploymentPushRoleArn

## Example Usage

```typescript
import { TargetDeploymentPushRoleArn } from "@alienplatform/platform-api/models";

let value: TargetDeploymentPushRoleArn = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                      | [models.TargetDeploymentPushRoleArnSecretRef](../models/targetdeploymentpushrolearnsecretref.md) | :heavy_check_mark:                                                                               | Reference to a Kubernetes Secret                                                                 |