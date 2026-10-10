# ApproveAccessRequestResponse

The approved access request.

## Example Usage

```typescript
import { ApproveAccessRequestResponse } from "@alienplatform/platform-api/models/operations";

let value: ApproveAccessRequestResponse = {
  id: "<id>",
  requesterKind: "serviceAccount",
  requesterId: "<id>",
  requestedExpiresAt: null,
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
  maxRisk: null,
  debugGrant: {
    tool: "gcloud",
    namespace: "braintrust",
    cloudScope: "123456789012/prod-readonly",
  },
  status: "pending-approval",
  approvedUntil: "<value>",
  createdAt: "1707614278049",
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
    actorKind: "serviceAccount",
    actorId: "<id>",
    at: "<value>",
    reason: "<value>",
  },
  approvalMethod: "slack",
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  | Example                                                                                                      |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `approvalChannels`                                                                                           | [models.AccessRequestApprovalChannel](../../models/accessrequestapprovalchannel.md)[]                        | :heavy_minus_sign:                                                                                           | N/A                                                                                                          |                                                                                                              |
| `id`                                                                                                         | *string*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |                                                                                                              |
| `requesterKind`                                                                                              | [operations.ApproveAccessRequestRequesterKind](../../models/operations/approveaccessrequestrequesterkind.md) | :heavy_check_mark:                                                                                           | N/A                                                                                                          |                                                                                                              |
| `requesterId`                                                                                                | *string*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |                                                                                                              |
| `requestedExpiresAt`                                                                                         | *string*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |                                                                                                              |
| `deploymentId`                                                                                               | *string*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |                                                                                                              |
| `deployment`                                                                                                 | [operations.ApproveAccessRequestDeployment](../../models/operations/approveaccessrequestdeployment.md)       | :heavy_minus_sign:                                                                                           | N/A                                                                                                          |                                                                                                              |
| `remediationPlanId`                                                                                          | *string*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |                                                                                                              |
| `agentSessionId`                                                                                             | *string*                                                                                                     | :heavy_check_mark:                                                                                           | The investigation whose remediation plan proposed this request, if a plan did.                               |                                                                                                              |
| `title`                                                                                                      | *string*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |                                                                                                              |
| `reason`                                                                                                     | *string*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |                                                                                                              |
| `commands`                                                                                                   | [operations.ApproveAccessRequestCommand](../../models/operations/approveaccessrequestcommand.md)[]           | :heavy_check_mark:                                                                                           | N/A                                                                                                          |                                                                                                              |
| `operationPattern`                                                                                           | *string*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |                                                                                                              |
| `maxRisk`                                                                                                    | [operations.ApproveAccessRequestMaxRisk](../../models/operations/approveaccessrequestmaxrisk.md)             | :heavy_check_mark:                                                                                           | How risky an operation is (declared by the plugin metadata).                                                 |                                                                                                              |
| `debugGrant`                                                                                                 | [models.AccessRequestDebugGrant](../../models/accessrequestdebuggrant.md)                                    | :heavy_check_mark:                                                                                           | N/A                                                                                                          |                                                                                                              |
| `status`                                                                                                     | [models.AccessRequestStatus](../../models/accessrequeststatus.md)                                            | :heavy_check_mark:                                                                                           | N/A                                                                                                          |                                                                                                              |
| `approvedUntil`                                                                                              | *string*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |                                                                                                              |
| `createdAt`                                                                                                  | *string*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |                                                                                                              |
| `queuedBy`                                                                                                   | *string*                                                                                                     | :heavy_check_mark:                                                                                           | Who passed the engineer gate; the requester for a plan-less request.                                         |                                                                                                              |
| `queuedAt`                                                                                                   | *string*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |                                                                                                              |
| `approvedBy`                                                                                                 | [operations.ApproveAccessRequestApprovedBy](../../models/operations/approveaccessrequestapprovedby.md)       | :heavy_check_mark:                                                                                           | How and when the customer gate was passed. Null until approved.                                              |                                                                                                              |
| `deniedBy`                                                                                                   | [operations.ApproveAccessRequestDeniedBy](../../models/operations/approveaccessrequestdeniedby.md)           | :heavy_check_mark:                                                                                           | N/A                                                                                                          |                                                                                                              |
| `revokedBy`                                                                                                  | [operations.ApproveAccessRequestRevokedBy](../../models/operations/approveaccessrequestrevokedby.md)         | :heavy_check_mark:                                                                                           | N/A                                                                                                          |                                                                                                              |
| `approvalMethod`                                                                                             | *string*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          | slack                                                                                                        |