# DenyAccessRequestResponse

The rejected access request.

## Example Usage

```typescript
import { DenyAccessRequestResponse } from "@alienplatform/platform-api/models/operations";

let value: DenyAccessRequestResponse = {
  id: "<id>",
  deploymentId: "<id>",
  deployment: {
    id: "<id>",
    name: "<value>",
    deploymentGroup: {
      id: "dg_r27ict8c7vcgsumpj90ackf7b",
      name: "prod-us-east-1",
      externalId: "ext_example_01",
    },
  },
  remediationPlanId: "<id>",
  title: "<value>",
  reason: "<value>",
  commands: [],
  operationPattern: null,
  maxRisk: null,
  status: "expired",
  approvedUntil: "<value>",
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `id`                                                                                             | *string*                                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `deploymentId`                                                                                   | *string*                                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `deployment`                                                                                     | [operations.DenyAccessRequestDeployment](../../models/operations/denyaccessrequestdeployment.md) | :heavy_minus_sign:                                                                               | N/A                                                                                              |
| `remediationPlanId`                                                                              | *string*                                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `title`                                                                                          | *string*                                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `reason`                                                                                         | *string*                                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `commands`                                                                                       | [operations.DenyAccessRequestCommand](../../models/operations/denyaccessrequestcommand.md)[]     | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `operationPattern`                                                                               | *string*                                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `maxRisk`                                                                                        | [operations.DenyAccessRequestMaxRisk](../../models/operations/denyaccessrequestmaxrisk.md)       | :heavy_check_mark:                                                                               | How risky an operation is (declared by the plugin metadata).                                     |
| `status`                                                                                         | [models.AccessRequestStatus](../../models/accessrequeststatus.md)                                | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `approvedUntil`                                                                                  | *string*                                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |