# DeploymentConfigBucketName1

## Example Usage

```typescript
import { DeploymentConfigBucketName1 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigBucketName1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                      | [models.DeploymentConfigBucketNameSecretRef1](../models/deploymentconfigbucketnamesecretref1.md) | :heavy_check_mark:                                                                               | Reference to a Kubernetes Secret                                                                 |