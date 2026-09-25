# ChildDeploymentPolicy

## Example Usage

```typescript
import { ChildDeploymentPolicy } from "@alienplatform/platform-api/models";

let value: ChildDeploymentPolicy = {
  enabled: true,
  maxChildren: 228785,
  allowedResources: [
    "container",
  ],
  allowedImages: [],
  allowedPoolIds: [
    "<value 1>",
    "<value 2>",
  ],
  maxCpuMillicoresPerChild: 900950,
  maxMemoryMiBPerChild: 897705,
  maxReplicasPerChild: 507227,
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `enabled`                                                | *boolean*                                                | :heavy_check_mark:                                       | N/A                                                      |
| `maxChildren`                                            | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `allowedResources`                                       | [models.AllowedResource](../models/allowedresource.md)[] | :heavy_check_mark:                                       | N/A                                                      |
| `allowedImages`                                          | *string*[]                                               | :heavy_check_mark:                                       | N/A                                                      |
| `allowedPoolIds`                                         | *string*[]                                               | :heavy_check_mark:                                       | N/A                                                      |
| `maxCpuMillicoresPerChild`                               | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `maxMemoryMiBPerChild`                                   | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `maxReplicasPerChild`                                    | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |