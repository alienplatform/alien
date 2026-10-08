# ListAccessRequestsItem

## Example Usage

```typescript
import { ListAccessRequestsItem } from "@alienplatform/platform-api/models/operations";

let value: ListAccessRequestsItem = {
  id: "<id>",
  requesterKind: "user",
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
  remediationPlanId: null,
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
  maxRisk: "destructive",
  debugGrant: null,
  status: "queued",
  approvedUntil: "<value>",
  createdAt: "1721284952451",
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
    reason: null,
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                     | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `requesterKind`                                                                                          | [operations.ListAccessRequestsRequesterKind](../../models/operations/listaccessrequestsrequesterkind.md) | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `requesterId`                                                                                            | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `requestedExpiresAt`                                                                                     | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `deploymentId`                                                                                           | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `deployment`                                                                                             | [operations.ListAccessRequestsDeployment](../../models/operations/listaccessrequestsdeployment.md)       | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `remediationPlanId`                                                                                      | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `agentSessionId`                                                                                         | *string*                                                                                                 | :heavy_check_mark:                                                                                       | The investigation whose remediation plan proposed this request, if a plan did.                           |
| `title`                                                                                                  | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `reason`                                                                                                 | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `commands`                                                                                               | [operations.ListAccessRequestsCommand](../../models/operations/listaccessrequestscommand.md)[]           | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `operationPattern`                                                                                       | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `maxRisk`                                                                                                | [operations.ListAccessRequestsMaxRisk](../../models/operations/listaccessrequestsmaxrisk.md)             | :heavy_check_mark:                                                                                       | How risky an operation is (declared by the plugin metadata).                                             |
| `debugGrant`                                                                                             | [models.AccessRequestDebugGrant](../../models/accessrequestdebuggrant.md)                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `status`                                                                                                 | [models.AccessRequestStatus](../../models/accessrequeststatus.md)                                        | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `approvedUntil`                                                                                          | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `createdAt`                                                                                              | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `queuedBy`                                                                                               | *string*                                                                                                 | :heavy_check_mark:                                                                                       | Who passed the engineer gate; the requester for a plan-less request.                                     |
| `queuedAt`                                                                                               | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `approvedBy`                                                                                             | [operations.ListAccessRequestsApprovedBy](../../models/operations/listaccessrequestsapprovedby.md)       | :heavy_check_mark:                                                                                       | How and when the customer gate was passed. Null until approved.                                          |
| `deniedBy`                                                                                               | [operations.ListAccessRequestsDeniedBy](../../models/operations/listaccessrequestsdeniedby.md)           | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `revokedBy`                                                                                              | [operations.ListAccessRequestsRevokedBy](../../models/operations/listaccessrequestsrevokedby.md)         | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
