# TargetDeploymentEnvironmentName

## Example Usage

```typescript
import { TargetDeploymentEnvironmentName } from "@alienplatform/platform-api/models";

let value: TargetDeploymentEnvironmentName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                              | [models.TargetDeploymentEnvironmentNameSecretRef](../models/targetdeploymentenvironmentnamesecretref.md) | :heavy_check_mark:                                                                                       | Reference to a Kubernetes Secret                                                                         |