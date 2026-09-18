# DeploymentConfigBucketName2

## Example Usage

```typescript
import { DeploymentConfigBucketName2 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigBucketName2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                      | [models.DeploymentConfigBucketNameSecretRef2](../models/deploymentconfigbucketnamesecretref2.md) | :heavy_check_mark:                                                                               | Reference to a Kubernetes Secret                                                                 |