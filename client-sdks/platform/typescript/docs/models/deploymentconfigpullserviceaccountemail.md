# DeploymentConfigPullServiceAccountEmail

## Example Usage

```typescript
import { DeploymentConfigPullServiceAccountEmail } from "@alienplatform/platform-api/models";

let value: DeploymentConfigPullServiceAccountEmail = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                              | [models.DeploymentConfigPullServiceAccountEmailSecretRef](../models/deploymentconfigpullserviceaccountemailsecretref.md) | :heavy_check_mark:                                                                                                       | Reference to a Kubernetes Secret                                                                                         |