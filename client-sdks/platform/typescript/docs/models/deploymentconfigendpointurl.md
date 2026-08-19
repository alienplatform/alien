# DeploymentConfigEndpointUrl

## Example Usage

```typescript
import { DeploymentConfigEndpointUrl } from "@alienplatform/platform-api/models";

let value: DeploymentConfigEndpointUrl = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                      | [models.DeploymentConfigEndpointUrlSecretRef](../models/deploymentconfigendpointurlsecretref.md) | :heavy_check_mark:                                                                               | Reference to a Kubernetes Secret                                                                 |