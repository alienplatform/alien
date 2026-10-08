# ListAccessRequestsDeployment

## Example Usage

```typescript
import { ListAccessRequestsDeployment } from "@alienplatform/platform-api/models/operations";

let value: ListAccessRequestsDeployment = {
  id: "<id>",
  name: "<value>",
  deploymentGroup: {
    id: "dg_r27ict8c7vcgsumpj90ackf7b",
    name: "prod-us-east-1",
    externalId: "ext_example_01",
  },
};
```

## Fields

| Field                                                             | Type                                                              | Required                                                          | Description                                                       |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `id`                                                              | *string*                                                          | :heavy_check_mark:                                                | N/A                                                               |
| `name`                                                            | *string*                                                          | :heavy_check_mark:                                                | N/A                                                               |
| `deploymentGroup`                                                 | [models.DeploymentGroupInfo](../../models/deploymentgroupinfo.md) | :heavy_minus_sign:                                                | N/A                                                               |