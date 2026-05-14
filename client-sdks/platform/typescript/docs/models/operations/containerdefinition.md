# ContainerDefinition

## Example Usage

```typescript
import { ContainerDefinition } from "@alienplatform/platform-api/models/operations";

let value: ContainerDefinition = {
  name: "<value>",
  totalInstances: 660708,
  runningInstances: 605148,
  failingInstances: 382885,
  pendingInstances: 115495,
  stoppedInstances: 373415,
  avgCpuPercent: 1614.75,
  avgMemoryPercent: 9918.9,
  totalReplicas: 69849,
  healthyReplicas: 577786,
  attentionCount: 853723,
  image: "https://loremflickr.com/2005/2726?lock=1309587520978016",
  stateful: false,
  hasAutoscaling: null,
  avgLatencyP95Ms: null,
  avgErrorRate: 5462.75,
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `name`                                                   | *string*                                                 | :heavy_check_mark:                                       | Container definition name (same across all deployments)  |
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