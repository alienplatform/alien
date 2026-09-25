# GetAccessRequestResponse

The access request.

## Example Usage

```typescript
import { GetAccessRequestResponse } from "@alienplatform/platform-api/models/operations";

let value: GetAccessRequestResponse = {
  id: "<id>",
  requesterKind: "user",
  requesterId: "<id>",
  requestedExpiresAt: "<value>",
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
  operationPattern: "<value>",
  maxRisk: "destructive",
  debugGrant: {
    tool: "gcloud",
    namespace: "braintrust",
    cloudScope: "123456789012/prod-readonly",
  },
  status: "customer-approved",
  approvedUntil: "<value>",
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `id`                                                                                                 | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `requesterKind`                                                                                      | [operations.GetAccessRequestRequesterKind](../../models/operations/getaccessrequestrequesterkind.md) | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `requesterId`                                                                                        | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `requestedExpiresAt`                                                                                 | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `deploymentId`                                                                                       | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `deployment`                                                                                         | [operations.GetAccessRequestDeployment](../../models/operations/getaccessrequestdeployment.md)       | :heavy_minus_sign:                                                                                   | N/A                                                                                                  |
| `remediationPlanId`                                                                                  | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `title`                                                                                              | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `reason`                                                                                             | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `commands`                                                                                           | [operations.GetAccessRequestCommand](../../models/operations/getaccessrequestcommand.md)[]           | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `operationPattern`                                                                                   | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `maxRisk`                                                                                            | [operations.GetAccessRequestMaxRisk](../../models/operations/getaccessrequestmaxrisk.md)             | :heavy_check_mark:                                                                                   | How risky an operation is (declared by the plugin metadata).                                         |
| `debugGrant`                                                                                         | [models.AccessRequestDebugGrant](../../models/accessrequestdebuggrant.md)                            | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `status`                                                                                             | [models.AccessRequestStatus](../../models/accessrequeststatus.md)                                    | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `approvedUntil`                                                                                      | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |