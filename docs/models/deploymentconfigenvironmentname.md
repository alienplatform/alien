# DeploymentConfigEnvironmentName

## Example Usage

```typescript
import { DeploymentConfigEnvironmentName } from "@alienplatform/platform-api/models";

let value: DeploymentConfigEnvironmentName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                              | [models.DeploymentConfigEnvironmentNameSecretRef](../models/deploymentconfigenvironmentnamesecretref.md) | :heavy_check_mark:                                                                                       | Reference to a Kubernetes Secret                                                                         |