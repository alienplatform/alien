# DeploymentInfoDeploymentGroup

Deployment group details (present when using a deployment-group token)

## Example Usage

```typescript
import { DeploymentInfoDeploymentGroup } from "@alienplatform/platform-api/models";

let value: DeploymentInfoDeploymentGroup = {
  id: "dg_r27ict8c7vcgsumpj90ackf7bnwt",
  name: "<value>",
  pinnedSubdomain: "<value>",
  maxDeployments: 611440,
  currentCount: 42267,
  lastDeletedDeployment: {
    id: "dep_0c29fq4a2yjb7kx3smwdgxlcxd7f",
    name: "<value>",
    completedAt: new Date("2026-01-13T19:16:00.092Z"),
  },
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        | Example                                                            |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `id`                                                               | *string*                                                           | :heavy_check_mark:                                                 | Unique identifier for the deployment group.                        | dg_r27ict8c7vcgsumpj90ackf7bnwt                                    |
| `name`                                                             | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |                                                                    |
| `pinnedSubdomain`                                                  | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |                                                                    |
| `maxDeployments`                                                   | *number*                                                           | :heavy_check_mark:                                                 | N/A                                                                |                                                                    |
| `currentCount`                                                     | *number*                                                           | :heavy_check_mark:                                                 | N/A                                                                |                                                                    |
| `lastDeletedDeployment`                                            | [models.LastDeletedDeployment](../models/lastdeleteddeployment.md) | :heavy_minus_sign:                                                 | N/A                                                                |                                                                    |