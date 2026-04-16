# ListDeploymentsResponse

## Example Usage

```typescript
import { ListDeploymentsResponse } from "@alienplatform/manager-api/models";

let value: ListDeploymentsResponse = {
  items: [
    {
      createdAt: "1716598022475",
      deploymentGroupId: "<id>",
      id: "<id>",
      name: "<value>",
      platform: "test",
      retryRequested: true,
      status: "<value>",
    },
  ],
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `items`                                                        | [models.DeploymentResponse](../models/deploymentresponse.md)[] | :heavy_check_mark:                                             | N/A                                                            |
| `nextCursor`                                                   | *string*                                                       | :heavy_minus_sign:                                             | N/A                                                            |