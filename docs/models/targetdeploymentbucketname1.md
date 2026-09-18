# TargetDeploymentBucketName1

## Example Usage

```typescript
import { TargetDeploymentBucketName1 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentBucketName1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                      | [models.TargetDeploymentBucketNameSecretRef1](../models/targetdeploymentbucketnamesecretref1.md) | :heavy_check_mark:                                                                               | Reference to a Kubernetes Secret                                                                 |