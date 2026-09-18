# DeploymentConfigPushServiceAccountEmail

## Example Usage

```typescript
import { DeploymentConfigPushServiceAccountEmail } from "@alienplatform/platform-api/models";

let value: DeploymentConfigPushServiceAccountEmail = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                              | [models.DeploymentConfigPushServiceAccountEmailSecretRef](../models/deploymentconfigpushserviceaccountemailsecretref.md) | :heavy_check_mark:                                                                                                       | Reference to a Kubernetes Secret                                                                                         |