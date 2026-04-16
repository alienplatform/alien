# ListDeploymentGroupsResponse

## Example Usage

```typescript
import { ListDeploymentGroupsResponse } from "@alienplatform/manager-api/models";

let value: ListDeploymentGroupsResponse = {
  items: [
    {
      createdAt: "1719218365065",
      deploymentCount: 193894,
      id: "<id>",
      maxDeployments: 468085,
      name: "<value>",
    },
  ],
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `items`                                                                  | [models.DeploymentGroupResponse](../models/deploymentgroupresponse.md)[] | :heavy_check_mark:                                                       | N/A                                                                      |
| `nextCursor`                                                             | *string*                                                                 | :heavy_minus_sign:                                                       | N/A                                                                      |