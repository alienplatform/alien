# ListResourceDeploymentsRequest

## Example Usage

```typescript
import { ListResourceDeploymentsRequest } from "@alienplatform/platform-api/models/operations";

let value: ListResourceDeploymentsRequest = {
  area: "daemon",
  resourceId: "<id>",
  project: "<value>",
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `area`                                                                                           | [operations.ListResourceDeploymentsArea](../../models/operations/listresourcedeploymentsarea.md) | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `resourceId`                                                                                     | *string*                                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `project`                                                                                        | *string*                                                                                         | :heavy_check_mark:                                                                               | Filter by project ID or name.                                                                    |
| `deploymentGroupId`                                                                              | *string*                                                                                         | :heavy_minus_sign:                                                                               | N/A                                                                                              |
| `deploymentId`                                                                                   | *string*                                                                                         | :heavy_minus_sign:                                                                               | N/A                                                                                              |