# MoveDeploymentRequest

## Example Usage

```typescript
import { MoveDeploymentRequest } from "@alienplatform/platform-api/models/operations";

let value: MoveDeploymentRequest = {
  id: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  moveDeploymentRequest: {
    deploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7b",
  },
};
```

## Fields

| Field                                                                 | Type                                                                  | Required                                                              | Description                                                           | Example                                                               |
| --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- |
| `id`                                                                  | *string*                                                              | :heavy_check_mark:                                                    | Unique identifier for the deployment.                                 | dep_0c29fq4a2yjb7kx3smwdgxlc                                          |
| `moveDeploymentRequest`                                               | [models.MoveDeploymentRequest](../../models/movedeploymentrequest.md) | :heavy_check_mark:                                                    | N/A                                                                   |                                                                       |
