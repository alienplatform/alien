# TargetDeploymentEndpointUrl

## Example Usage

```typescript
import { TargetDeploymentEndpointUrl } from "@alienplatform/platform-api/models";

let value: TargetDeploymentEndpointUrl = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                      | [models.TargetDeploymentEndpointUrlSecretRef](../models/targetdeploymentendpointurlsecretref.md) | :heavy_check_mark:                                                                               | Reference to a Kubernetes Secret                                                                 |