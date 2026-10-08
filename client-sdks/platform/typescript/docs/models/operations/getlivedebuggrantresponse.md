# GetLiveDebugGrantResponse

A live access request with a matching debug grant.

## Example Usage

```typescript
import { GetLiveDebugGrantResponse } from "@alienplatform/platform-api/models/operations";

let value: GetLiveDebugGrantResponse = {
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
  agentSessionId: "<id>",
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
  maxRisk: "mutating",
  debugGrant: null,
  status: "customer-approved",
  approvedUntil: "<value>",
  createdAt: "1727019644622",
  queuedBy: "<value>",
  queuedAt: "<value>",
  approvedBy: {
    method: "<value>",
    actorId: "<id>",
    at: "<value>",
  },
  deniedBy: {
    actorId: null,
    at: "<value>",
  },
  revokedBy: {
    actorKind: "user",
    actorId: "<id>",
    at: "<value>",
    reason: "<value>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `id`                                                                                                   | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `requesterKind`                                                                                        | [operations.GetLiveDebugGrantRequesterKind](../../models/operations/getlivedebuggrantrequesterkind.md) | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `requesterId`                                                                                          | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `requestedExpiresAt`                                                                                   | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `deploymentId`                                                                                         | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `deployment`                                                                                           | [operations.GetLiveDebugGrantDeployment](../../models/operations/getlivedebuggrantdeployment.md)       | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `remediationPlanId`                                                                                    | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `agentSessionId`                                                                                       | *string*                                                                                               | :heavy_check_mark:                                                                                     | The investigation whose remediation plan proposed this request, if a plan did.                         |
| `title`                                                                                                | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `reason`                                                                                               | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `commands`                                                                                             | [operations.GetLiveDebugGrantCommand](../../models/operations/getlivedebuggrantcommand.md)[]           | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `operationPattern`                                                                                     | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `maxRisk`                                                                                              | [operations.GetLiveDebugGrantMaxRisk](../../models/operations/getlivedebuggrantmaxrisk.md)             | :heavy_check_mark:                                                                                     | How risky an operation is (declared by the plugin metadata).                                           |
| `debugGrant`                                                                                           | [models.AccessRequestDebugGrant](../../models/accessrequestdebuggrant.md)                              | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `status`                                                                                               | [models.AccessRequestStatus](../../models/accessrequeststatus.md)                                      | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `approvedUntil`                                                                                        | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `createdAt`                                                                                            | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `queuedBy`                                                                                             | *string*                                                                                               | :heavy_check_mark:                                                                                     | Who passed the engineer gate; the requester for a plan-less request.                                   |
| `queuedAt`                                                                                             | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `approvedBy`                                                                                           | [operations.GetLiveDebugGrantApprovedBy](../../models/operations/getlivedebuggrantapprovedby.md)       | :heavy_check_mark:                                                                                     | How and when the customer gate was passed. Null until approved.                                        |
| `deniedBy`                                                                                             | [operations.GetLiveDebugGrantDeniedBy](../../models/operations/getlivedebuggrantdeniedby.md)           | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `revokedBy`                                                                                            | [operations.GetLiveDebugGrantRevokedBy](../../models/operations/getlivedebuggrantrevokedby.md)         | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
