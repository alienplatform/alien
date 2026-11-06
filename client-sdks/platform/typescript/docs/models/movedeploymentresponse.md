# MoveDeploymentResponse

## Example Usage

```typescript
import { MoveDeploymentResponse } from "@alienplatform/platform-api/models";

let value: MoveDeploymentResponse = {
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlcxd7f",
  previousDeploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7bnwt",
  deploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7bnwt",
  membershipRevision: 552519,
  result: "unchanged",
  blockers: [
    "<value 1>",
    "<value 2>",
  ],
  projectionStatus: "not-required",
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              | Example                                                  |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `deploymentId`                                           | *string*                                                 | :heavy_check_mark:                                       | Unique identifier for the deployment.                    | dep_0c29fq4a2yjb7kx3smwdgxlcxd7f                         |
| `previousDeploymentGroupId`                              | *string*                                                 | :heavy_check_mark:                                       | Unique identifier for the deployment group.              | dg_r27ict8c7vcgsumpj90ackf7bnwt                          |
| `deploymentGroupId`                                      | *string*                                                 | :heavy_check_mark:                                       | Unique identifier for the deployment group.              | dg_r27ict8c7vcgsumpj90ackf7bnwt                          |
| `membershipRevision`                                     | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |                                                          |
| `result`                                                 | [models.Result](../models/result.md)                     | :heavy_check_mark:                                       | N/A                                                      |                                                          |
| `blockers`                                               | *string*[]                                               | :heavy_check_mark:                                       | N/A                                                      |                                                          |
| `projectionStatus`                                       | [models.ProjectionStatus](../models/projectionstatus.md) | :heavy_check_mark:                                       | N/A                                                      |                                                          |