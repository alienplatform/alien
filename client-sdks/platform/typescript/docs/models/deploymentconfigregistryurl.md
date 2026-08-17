# DeploymentConfigRegistryUrl

## Example Usage

```typescript
import { DeploymentConfigRegistryUrl } from "@alienplatform/platform-api/models";

let value: DeploymentConfigRegistryUrl = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                      | [models.DeploymentConfigRegistryUrlSecretRef](../models/deploymentconfigregistryurlsecretref.md) | :heavy_check_mark:                                                                               | Reference to a Kubernetes Secret                                                                 |