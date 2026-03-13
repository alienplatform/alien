# ListDeploymentGroupsResponse

Paginated response

## Example Usage

```typescript
import { ListDeploymentGroupsResponse } from "@aliendotdev/platform-api/models/operations";

let value: ListDeploymentGroupsResponse = {
  items: [
    {
      id: "dg_r27ict8c7vcgsumpj90ackf7b",
      name: "prod-us-east-1",
      projectId: "prj_mcytp6z3j91f7tn5ryqsfwtr",
      workspaceId: "ws_It13CUaGEhLLAB87simX0",
      createdAt: new Date("2025-06-07T13:26:09.959Z"),
    },
  ],
  nextCursor: "<value>",
};
```

## Fields

| Field                                                       | Type                                                        | Required                                                    | Description                                                 |
| ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- |
| `items`                                                     | [models.DeploymentGroup](../../models/deploymentgroup.md)[] | :heavy_check_mark:                                          | Items in this page                                          |
| `nextCursor`                                                | *string*                                                    | :heavy_check_mark:                                          | Cursor for the next page, null if last page                 |