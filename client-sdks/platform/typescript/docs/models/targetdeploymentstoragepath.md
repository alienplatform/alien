# TargetDeploymentStoragePath

## Example Usage

```typescript
import { TargetDeploymentStoragePath } from "@alienplatform/platform-api/models";

let value: TargetDeploymentStoragePath = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                      | [models.TargetDeploymentStoragePathSecretRef](../models/targetdeploymentstoragepathsecretref.md) | :heavy_check_mark:                                                                               | Reference to a Kubernetes Secret                                                                 |