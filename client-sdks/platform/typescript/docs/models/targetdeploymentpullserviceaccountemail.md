# TargetDeploymentPullServiceAccountEmail

## Example Usage

```typescript
import { TargetDeploymentPullServiceAccountEmail } from "@alienplatform/platform-api/models";

let value: TargetDeploymentPullServiceAccountEmail = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                              | [models.TargetDeploymentPullServiceAccountEmailSecretRef](../models/targetdeploymentpullserviceaccountemailsecretref.md) | :heavy_check_mark:                                                                                                       | Reference to a Kubernetes Secret                                                                                         |