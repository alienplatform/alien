# ApproveAccessRequestResponse

The approved access request.

## Example Usage

```typescript
import { ApproveAccessRequestResponse } from "@alienplatform/platform-api/models/operations";

let value: ApproveAccessRequestResponse = {
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
  commands: [
    {
      command: "kubernetes/get-pods",
      summary: "List pods in the ingestion namespace",
      params: {
        "pod": "ingester-p4kwm",
      },
    },
  ],
  operationPattern: null,
  maxRisk: "mutating",
  status: "rejected",
  approvedUntil: "<value>",
  approvalMethod: "slack",
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            | Example                                                                                                |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `id`                                                                                                   | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |                                                                                                        |
| `deploymentId`                                                                                         | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |                                                                                                        |
| `deployment`                                                                                           | [operations.ApproveAccessRequestDeployment](../../models/operations/approveaccessrequestdeployment.md) | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |                                                                                                        |
| `remediationPlanId`                                                                                    | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |                                                                                                        |
| `title`                                                                                                | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |                                                                                                        |
| `reason`                                                                                               | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |                                                                                                        |
| `commands`                                                                                             | [operations.ApproveAccessRequestCommand](../../models/operations/approveaccessrequestcommand.md)[]     | :heavy_check_mark:                                                                                     | N/A                                                                                                    |                                                                                                        |
| `operationPattern`                                                                                     | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |                                                                                                        |
| `maxRisk`                                                                                              | [operations.ApproveAccessRequestMaxRisk](../../models/operations/approveaccessrequestmaxrisk.md)       | :heavy_check_mark:                                                                                     | How risky an operation is (declared by the plugin metadata).                                           |                                                                                                        |
| `status`                                                                                               | [models.AccessRequestStatus](../../models/accessrequeststatus.md)                                      | :heavy_check_mark:                                                                                     | N/A                                                                                                    |                                                                                                        |
| `approvedUntil`                                                                                        | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |                                                                                                        |
| `approvalMethod`                                                                                       | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    | slack                                                                                                  |