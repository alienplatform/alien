# TargetDeploymentPushServiceAccountEmail

## Example Usage

```typescript
import { TargetDeploymentPushServiceAccountEmail } from "@alienplatform/platform-api/models";

let value: TargetDeploymentPushServiceAccountEmail = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                              | [models.TargetDeploymentPushServiceAccountEmailSecretRef](../models/targetdeploymentpushserviceaccountemailsecretref.md) | :heavy_check_mark:                                                                                                       | Reference to a Kubernetes Secret                                                                                         |