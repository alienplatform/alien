# TargetDeploymentBucketName2

## Example Usage

```typescript
import { TargetDeploymentBucketName2 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentBucketName2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                      | [models.TargetDeploymentBucketNameSecretRef2](../models/targetdeploymentbucketnamesecretref2.md) | :heavy_check_mark:                                                                               | Reference to a Kubernetes Secret                                                                 |