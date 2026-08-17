# DeploymentConfigRegion

## Example Usage

```typescript
import { DeploymentConfigRegion } from "@alienplatform/platform-api/models";

let value: DeploymentConfigRegion = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `secretRef`                                                                            | [models.DeploymentConfigRegionSecretRef](../models/deploymentconfigregionsecretref.md) | :heavy_check_mark:                                                                     | Reference to a Kubernetes Secret                                                       |