# CreateAccessRequestResponse

The created access request.

## Example Usage

```typescript
import { CreateAccessRequestResponse } from "@alienplatform/platform-api/models/operations";

let value: CreateAccessRequestResponse = {
  id: "<id>",
  requesterKind: "serviceAccount",
  requesterId: "<id>",
  requestedExpiresAt: "<value>",
  deploymentId: "<id>",
  deployment: {
    id: "<id>",
    name: "<value>",
    deploymentGroup: {
      id: "dg_r27ict8c7vcgsumpj90ackf7bnwt",
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
  debugGrant: {
    tool: "gcloud",
    namespace: "braintrust",
    cloudScope: "123456789012/prod-readonly",
  },
  status: "expired",
  approvedUntil: "<value>",
  createdAt: "1716409325860",
  queuedBy: "<value>",
  queuedAt: "<value>",
  approver: "customer",
  approvedBy: {
    method: null,
    actorId: "<id>",
    slack: {
      userId: "<id>",
      userName: "Dusty_Simonis",
      teamId: "<id>",
      teamName: "<value>",
      channelId: "<id>",
      messageTs: "<value>",
      app: "<value>",
    },
    at: "<value>",
  },
  deniedBy: {
    actorId: "<id>",
    slack: {
      userId: "<id>",
      userName: "Dusty_Simonis",
      teamId: "<id>",
      teamName: "<value>",
      channelId: "<id>",
      messageTs: "<value>",
      app: "<value>",
    },
    at: "<value>",
  },
  revokedBy: null,
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                       | *string*                                                                                                   | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `requesterKind`                                                                                            | [operations.CreateAccessRequestRequesterKind](../../models/operations/createaccessrequestrequesterkind.md) | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `requesterId`                                                                                              | *string*                                                                                                   | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `requestedExpiresAt`                                                                                       | *string*                                                                                                   | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `deploymentId`                                                                                             | *string*                                                                                                   | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `deployment`                                                                                               | [operations.CreateAccessRequestDeployment](../../models/operations/createaccessrequestdeployment.md)       | :heavy_minus_sign:                                                                                         | N/A                                                                                                        |
| `remediationPlanId`                                                                                        | *string*                                                                                                   | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `agentSessionId`                                                                                           | *string*                                                                                                   | :heavy_check_mark:                                                                                         | The investigation whose remediation plan proposed this request, if a plan did.                             |
| `title`                                                                                                    | *string*                                                                                                   | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `reason`                                                                                                   | *string*                                                                                                   | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `commands`                                                                                                 | [operations.CreateAccessRequestCommand](../../models/operations/createaccessrequestcommand.md)[]           | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `operationPattern`                                                                                         | *string*                                                                                                   | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `maxRisk`                                                                                                  | [operations.CreateAccessRequestMaxRisk](../../models/operations/createaccessrequestmaxrisk.md)             | :heavy_check_mark:                                                                                         | How risky an operation is (declared by the plugin metadata).                                               |
| `debugGrant`                                                                                               | [models.AccessRequestDebugGrant](../../models/accessrequestdebuggrant.md)                                  | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `status`                                                                                                   | [models.AccessRequestStatus](../../models/accessrequeststatus.md)                                          | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `approvedUntil`                                                                                            | *string*                                                                                                   | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `createdAt`                                                                                                | *string*                                                                                                   | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `queuedBy`                                                                                                 | *string*                                                                                                   | :heavy_check_mark:                                                                                         | Who passed the engineer gate; the requester for a plan-less request.                                       |
| `queuedAt`                                                                                                 | *string*                                                                                                   | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `approver`                                                                                                 | [models.AccessApprover](../../models/accessapprover.md)                                                    | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `approvedBy`                                                                                               | [operations.CreateAccessRequestApprovedBy](../../models/operations/createaccessrequestapprovedby.md)       | :heavy_check_mark:                                                                                         | How, when and by whom the request was approved. Null until approved.                                       |
| `deniedBy`                                                                                                 | [operations.CreateAccessRequestDeniedBy](../../models/operations/createaccessrequestdeniedby.md)           | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `revokedBy`                                                                                                | [operations.CreateAccessRequestRevokedBy](../../models/operations/createaccessrequestrevokedby.md)         | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `approvalChannels`                                                                                         | [models.AccessRequestApprovalChannel](../../models/accessrequestapprovalchannel.md)[]                      | :heavy_minus_sign:                                                                                         | N/A                                                                                                        |