# DeploymentGroup

## Example Usage

```typescript
import { DeploymentGroup } from "@alienplatform/platform-api/models";

let value: DeploymentGroup = {
  id: "dg_r27ict8c7vcgsumpj90ackf7b",
  name: "prod-us-east-1",
  projectId: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  workspaceId: "ws_It13CUaGEhLLAB87simX0",
  createdAt: new Date("2025-09-26T07:50:12.593Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the deployment group.                                                   | dg_r27ict8c7vcgsumpj90ackf7b                                                                  |
| `name`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | Deployment group name.                                                                        | prod-us-east-1                                                                                |
| `projectId`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the project.                                                            | prj_mcytp6z3j91f7tn5ryqsfwtr                                                                  |
| `workspaceId`                                                                                 | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the workspace.                                                          | ws_It13CUaGEhLLAB87simX0                                                                      |
| `maxDeployments`                                                                              | *number*                                                                                      | :heavy_minus_sign:                                                                            | Maximum number of deployments allowed in this deployment group                                |                                                                                               |
| `createdAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |