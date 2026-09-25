# ChildDeploymentPolicyResponse

## Example Usage

```typescript
import { ChildDeploymentPolicyResponse } from "@alienplatform/platform-api/models";

let value: ChildDeploymentPolicyResponse = {
  version: 85293,
  policy: {
    enabled: false,
    maxChildren: 891140,
    allowedResources: [
      "container",
    ],
    allowedImages: [
      "<value 1>",
      "<value 2>",
    ],
    allowedPoolIds: [
      "<value 1>",
      "<value 2>",
    ],
    maxCpuMillicoresPerChild: 878430,
    maxMemoryMiBPerChild: 171842,
    maxReplicasPerChild: 969267,
  },
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `version`                                                          | *number*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `policy`                                                           | [models.ChildDeploymentPolicy](../models/childdeploymentpolicy.md) | :heavy_check_mark:                                                 | N/A                                                                |