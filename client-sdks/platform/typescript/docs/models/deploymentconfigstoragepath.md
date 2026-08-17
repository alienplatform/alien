# DeploymentConfigStoragePath

## Example Usage

```typescript
import { DeploymentConfigStoragePath } from "@alienplatform/platform-api/models";

let value: DeploymentConfigStoragePath = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                      | [models.DeploymentConfigStoragePathSecretRef](../models/deploymentconfigstoragepathsecretref.md) | :heavy_check_mark:                                                                               | Reference to a Kubernetes Secret                                                                 |