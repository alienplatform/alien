# GetContainerDefinitionDeploymentsDeployment

## Example Usage

```typescript
import { GetContainerDefinitionDeploymentsDeployment } from "@aliendotdev/platform-api/models/operations";

let value: GetContainerDefinitionDeploymentsDeployment = {
  clusterId: "<id>",
  deploymentId: "<id>",
  deploymentName: "<value>",
  status: "<value>",
  statusReason: "<value>",
  statusMessage: "<value>",
  image: "https://picsum.photos/seed/07Jed/416/2747",
  currentReplicas: 355432,
  healthyReplicas: 659749,
  avgCpuPercent: 3408.27,
  avgMemoryPercent: 9745.08,
  avgLatencyP95Ms: 7535.52,
  avgErrorRate: 7602.62,
  avgInFlightRequests: 3851.74,
};
```

## Fields

| Field                                     | Type                                      | Required                                  | Description                               |
| ----------------------------------------- | ----------------------------------------- | ----------------------------------------- | ----------------------------------------- |
| `clusterId`                               | *string*                                  | :heavy_check_mark:                        | N/A                                       |
| `deploymentId`                            | *string*                                  | :heavy_check_mark:                        | Deployment ID for building deep-link URLs |
| `deploymentName`                          | *string*                                  | :heavy_check_mark:                        | Deployment name for display               |
| `deploymentGroupId`                       | *string*                                  | :heavy_minus_sign:                        | N/A                                       |
| `deploymentGroupName`                     | *string*                                  | :heavy_minus_sign:                        | N/A                                       |
| `projectName`                             | *string*                                  | :heavy_minus_sign:                        | N/A                                       |
| `status`                                  | *string*                                  | :heavy_check_mark:                        | N/A                                       |
| `statusReason`                            | *string*                                  | :heavy_check_mark:                        | N/A                                       |
| `statusMessage`                           | *string*                                  | :heavy_check_mark:                        | N/A                                       |
| `image`                                   | *string*                                  | :heavy_check_mark:                        | N/A                                       |
| `currentReplicas`                         | *number*                                  | :heavy_check_mark:                        | N/A                                       |
| `healthyReplicas`                         | *number*                                  | :heavy_check_mark:                        | N/A                                       |
| `avgCpuPercent`                           | *number*                                  | :heavy_check_mark:                        | N/A                                       |
| `avgMemoryPercent`                        | *number*                                  | :heavy_check_mark:                        | N/A                                       |
| `avgLatencyP95Ms`                         | *number*                                  | :heavy_check_mark:                        | N/A                                       |
| `avgErrorRate`                            | *number*                                  | :heavy_check_mark:                        | N/A                                       |
| `avgInFlightRequests`                     | *number*                                  | :heavy_check_mark:                        | N/A                                       |