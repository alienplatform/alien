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
  agentSessionId: "<id>",
  title: "<value>",
  reason: "<value>",
  commands: [],
  operationPattern: "<value>",
  maxRisk: "mutating",
  debugGrant: null,
  status: "revoked",
  approvedUntil: "<value>",
  createdAt: "1728064373891",
  queuedBy: "<value>",
  queuedAt: "<value>",
  approvedBy: {
    method: "<value>",
    actorId: "<id>",
    at: "<value>",
  },
  deniedBy: {
    actorId: "<id>",
    at: "<value>",
  },
  revokedBy: {
    actorKind: "user",
    actorId: "<id>",
    at: "<value>",
    reason: "<value>",
  },
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
| `agentSessionId`                                                                                         | *string*                                                                                                 | :heavy_check_mark:                                                                                       | The investigation whose remediation plan proposed this request, if a plan did.                           |
| `title`                                                                                                  | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `reason`                                                                                                 | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `commands`                                                                                               | [operations.QueueAccessRequestCommand](../../models/operations/queueaccessrequestcommand.md)[]           | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `operationPattern`                                                                                       | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `maxRisk`                                                                                                | [operations.QueueAccessRequestMaxRisk](../../models/operations/queueaccessrequestmaxrisk.md)             | :heavy_check_mark:                                                                                       | How risky an operation is (declared by the plugin metadata).                                             |
| `debugGrant`                                                                                             | [models.AccessRequestDebugGrant](../../models/accessrequestdebuggrant.md)                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `status`                                                                                                 | [models.AccessRequestStatus](../../models/accessrequeststatus.md)                                        | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `approvedUntil`                                                                                          | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `createdAt`                                                                                              | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `queuedBy`                                                                                               | *string*                                                                                                 | :heavy_check_mark:                                                                                       | Who passed the engineer gate; the requester for a plan-less request.                                     |
| `queuedAt`                                                                                               | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `approvedBy`                                                                                             | [operations.QueueAccessRequestApprovedBy](../../models/operations/queueaccessrequestapprovedby.md)       | :heavy_check_mark:                                                                                       | How and when the customer gate was passed. Null until approved.                                          |
| `deniedBy`                                                                                               | [operations.QueueAccessRequestDeniedBy](../../models/operations/queueaccessrequestdeniedby.md)           | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `revokedBy`                                                                                              | [operations.QueueAccessRequestRevokedBy](../../models/operations/queueaccessrequestrevokedby.md)         | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `approvalChannels`                                                                                       | [models.AccessRequestApprovalChannel](../../models/accessrequestapprovalchannel.md)[]                    | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `kubectlApprove`                                                                                         | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
