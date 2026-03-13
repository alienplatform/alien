# ContainerDefinition

## Example Usage

```typescript
import { ContainerDefinition } from "@alienplatform/platform-api/models/operations";

let value: ContainerDefinition = {
  name: "<value>",
  projectId: "<id>",
  projectName: "<value>",
  totalInstances: 382885,
  runningInstances: 115495,
  failingInstances: 373415,
  pendingInstances: 533962,
  stoppedInstances: 161475,
  avgCpuPercent: 9918.9,
  avgMemoryPercent: null,
  totalReplicas: 577786,
  healthyReplicas: 853723,
  attentionCount: 131071,
  image: "https://loremflickr.com/2726/1266?lock=3189269540663527",
  stateful: true,
  hasAutoscaling: null,
  avgLatencyP95Ms: 5462.75,
  avgErrorRate: 3582.68,
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `name`                                                   | *string*                                                 | :heavy_check_mark:                                       | Container definition name (same across all deployments)  |
| `projectId`                                              | *string*                                                 | :heavy_check_mark:                                       | Project ID this container belongs to                     |
| `projectName`                                            | *string*                                                 | :heavy_check_mark:                                       | Project name for display (null at workspace level)       |
| `totalInstances`                                         | *number*                                                 | :heavy_check_mark:                                       | Total deployments running this container                 |
| `runningInstances`                                       | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `failingInstances`                                       | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `pendingInstances`                                       | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `stoppedInstances`                                       | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `avgCpuPercent`                                          | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `avgMemoryPercent`                                       | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `totalReplicas`                                          | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `healthyReplicas`                                        | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `attentionCount`                                         | *number*                                                 | :heavy_check_mark:                                       | Deployments with issues (failing or scheduling failures) |
| `image`                                                  | *string*                                                 | :heavy_check_mark:                                       | Container image from a representative deployment         |
| `stateful`                                               | *boolean*                                                | :heavy_check_mark:                                       | N/A                                                      |
| `hasAutoscaling`                                         | *boolean*                                                | :heavy_check_mark:                                       | N/A                                                      |
| `avgLatencyP95Ms`                                        | *number*                                                 | :heavy_check_mark:                                       | Average p95 HTTP latency across replicas                 |
| `avgErrorRate`                                           | *number*                                                 | :heavy_check_mark:                                       | Average HTTP error rate (5xx/total), 0-1                 |