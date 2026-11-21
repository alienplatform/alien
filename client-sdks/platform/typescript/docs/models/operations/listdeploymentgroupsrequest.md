# ListDeploymentGroupsRequest

## Example Usage

```typescript
import { ListDeploymentGroupsRequest } from "@alienplatform/platform-api/models/operations";

let value: ListDeploymentGroupsRequest = {};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `project`                                                                                          | *string*                                                                                           | :heavy_minus_sign:                                                                                 | Filter by project ID or name.                                                                      |
| `pendingInvites`                                                                                   | [operations.PendingInvites](../../models/operations/pendinginvites.md)                             | :heavy_minus_sign:                                                                                 | N/A                                                                                                |
| `empty`                                                                                            | [operations.Empty](../../models/operations/empty.md)                                               | :heavy_minus_sign:                                                                                 | Only groups without deployments                                                                    |
| `search`                                                                                           | *string*                                                                                           | :heavy_minus_sign:                                                                                 | Search deployment groups by name                                                                   |
| `include`                                                                                          | [operations.ListDeploymentGroupsInclude](../../models/operations/listdeploymentgroupsinclude.md)[] | :heavy_minus_sign:                                                                                 | Optional fields to include: project                                                                |
| `limit`                                                                                            | *number*                                                                                           | :heavy_minus_sign:                                                                                 | Maximum number of items to return per page                                                         |
| `cursor`                                                                                           | *string*                                                                                           | :heavy_minus_sign:                                                                                 | Cursor for pagination - omit for first page                                                        |