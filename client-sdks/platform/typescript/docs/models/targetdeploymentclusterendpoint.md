# TargetDeploymentClusterEndpoint

## Example Usage

```typescript
import { TargetDeploymentClusterEndpoint } from "@alienplatform/platform-api/models";

let value: TargetDeploymentClusterEndpoint = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                              | [models.TargetDeploymentClusterEndpointSecretRef](../models/targetdeploymentclusterendpointsecretref.md) | :heavy_check_mark:                                                                                       | Reference to a Kubernetes Secret                                                                         |