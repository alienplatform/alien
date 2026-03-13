# ListDeploymentFilterDeploymentGroupsItem

## Example Usage

```typescript
import { ListDeploymentFilterDeploymentGroupsItem } from "@alienplatform/platform-api/models/operations";

let value: ListDeploymentFilterDeploymentGroupsItem = {
  id: "dg_r27ict8c7vcgsumpj90ackf7b",
  name: "<value>",
  deploymentCount: 5864.14,
  runningCount: 2831.71,
  failedCount: 6488.27,
  inProgressCount: 9465.94,
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              | Example                                                                  |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `id`                                                                     | *string*                                                                 | :heavy_check_mark:                                                       | Unique identifier for the deployment group.                              | dg_r27ict8c7vcgsumpj90ackf7b                                             |
| `name`                                                                   | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |                                                                          |
| `deploymentCount`                                                        | *number*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |                                                                          |
| `runningCount`                                                           | *number*                                                                 | :heavy_check_mark:                                                       | Number of agents in 'running' status                                     |                                                                          |
| `failedCount`                                                            | *number*                                                                 | :heavy_check_mark:                                                       | Number of agents in a failed status                                      |                                                                          |
| `inProgressCount`                                                        | *number*                                                                 | :heavy_check_mark:                                                       | Number of agents in an in-progress status (provisioning, updating, etc.) |                                                                          |