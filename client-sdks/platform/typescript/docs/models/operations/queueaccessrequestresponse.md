# QueueAccessRequestResponse

The queued access request, with the customer approve command.

## Example Usage

```typescript
import { QueueAccessRequestResponse } from "@alienplatform/platform-api/models/operations";

let value: QueueAccessRequestResponse = {
  id: "<id>",
  requesterKind: "serviceAccount",
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
  commands: [],
  operationPattern: null,
  maxRisk: "mutating",
  debugGrant: {
    tool: "kubectl",
    namespace: "braintrust",
    cloudScope: "123456789012/prod-readonly",
  },
  status: "pending-approval",
  approvedUntil: "<value>",
  kubectlApprove: "<value>",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                     | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `requesterKind`                                                                                          | [operations.QueueAccessRequestRequesterKind](../../models/operations/queueaccessrequestrequesterkind.md) | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `requesterId`                                                                                            | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `requestedExpiresAt`                                                                                     | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `deploymentId`                                                                                           | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `deployment`                                                                                             | [operations.QueueAccessRequestDeployment](../../models/operations/queueaccessrequestdeployment.md)       | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `remediationPlanId`                                                                                      | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `title`                                                                                                  | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `reason`                                                                                                 | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `commands`                                                                                               | [operations.QueueAccessRequestCommand](../../models/operations/queueaccessrequestcommand.md)[]           | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `operationPattern`                                                                                       | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `maxRisk`                                                                                                | [operations.QueueAccessRequestMaxRisk](../../models/operations/queueaccessrequestmaxrisk.md)             | :heavy_check_mark:                                                                                       | How risky an operation is (declared by the plugin metadata).                                             |
| `debugGrant`                                                                                             | [models.AccessRequestDebugGrant](../../models/accessrequestdebuggrant.md)                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `status`                                                                                                 | [models.AccessRequestStatus](../../models/accessrequeststatus.md)                                        | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `approvedUntil`                                                                                          | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `kubectlApprove`                                                                                         | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |