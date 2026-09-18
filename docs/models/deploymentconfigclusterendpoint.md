# DeploymentConfigClusterEndpoint

## Example Usage

```typescript
import { DeploymentConfigClusterEndpoint } from "@alienplatform/platform-api/models";

let value: DeploymentConfigClusterEndpoint = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                              | [models.DeploymentConfigClusterEndpointSecretRef](../models/deploymentconfigclusterendpointsecretref.md) | :heavy_check_mark:                                                                                       | Reference to a Kubernetes Secret                                                                         |