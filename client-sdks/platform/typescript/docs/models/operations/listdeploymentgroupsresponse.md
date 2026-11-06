# ListDeploymentGroupsResponse

List of deployment groups

## Example Usage

```typescript
import { ListDeploymentGroupsResponse } from "@alienplatform/platform-api/models/operations";

let value: ListDeploymentGroupsResponse = {
  items: [
    {
      id: "dg_r27ict8c7vcgsumpj90ackf7bnwt",
      name: "prod-us-east-1",
      externalId: "ext_example_01",
      projectId: "prj_mcytp6z3j91f7tn5ryqsfwtrdqkv",
      workspaceId: "ws_It13CUaGEhLLAB87simX07Xb",
      createdAt: new Date("2025-06-07T13:26:09.959Z"),
    },
  ],
  nextCursor: "<value>",
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `items`                                                                                            | [operations.ListDeploymentGroupsItem](../../models/operations/listdeploymentgroupsitem.md)[]       | :heavy_check_mark:                                                                                 | Items in this page                                                                                 |
| `nextCursor`                                                                                       | *string*                                                                                           | :heavy_check_mark:                                                                                 | Cursor for the next page, null if last page                                                        |
| `total`                                                                                            | *number*                                                                                           | :heavy_minus_sign:                                                                                 | Total matching empty or invited deployment groups; returned to workspace users and administrators. |