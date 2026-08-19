# TargetDeploymentRegistryUrl

## Example Usage

```typescript
import { TargetDeploymentRegistryUrl } from "@alienplatform/platform-api/models";

let value: TargetDeploymentRegistryUrl = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                      | [models.TargetDeploymentRegistryUrlSecretRef](../models/targetdeploymentregistryurlsecretref.md) | :heavy_check_mark:                                                                               | Reference to a Kubernetes Secret                                                                 |