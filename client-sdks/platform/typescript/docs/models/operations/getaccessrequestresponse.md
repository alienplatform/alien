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
  maxRisk: "read-only",
  debugGrant: {
    tool: "gcloud",
    namespace: "braintrust",
    cloudScope: "123456789012/prod-readonly",
  },
  status: "rejected",
  approvedUntil: "<value>",
  createdAt: "1724419880003",
  queuedBy: "<value>",
  queuedAt: "<value>",
  approvedBy: {
    method: null,
    actorId: null,
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

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `id`                                                                                                 | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `requesterKind`                                                                                      | [operations.GetAccessRequestRequesterKind](../../models/operations/getaccessrequestrequesterkind.md) | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `requesterId`                                                                                        | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `requestedExpiresAt`                                                                                 | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `deploymentId`                                                                                       | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `deployment`                                                                                         | [operations.GetAccessRequestDeployment](../../models/operations/getaccessrequestdeployment.md)       | :heavy_minus_sign:                                                                                   | N/A                                                                                                  |
| `remediationPlanId`                                                                                  | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `agentSessionId`                                                                                     | *string*                                                                                             | :heavy_check_mark:                                                                                   | The investigation whose remediation plan proposed this request, if a plan did.                       |
| `title`                                                                                              | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `reason`                                                                                             | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `commands`                                                                                           | [operations.GetAccessRequestCommand](../../models/operations/getaccessrequestcommand.md)[]           | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `operationPattern`                                                                                   | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `maxRisk`                                                                                            | [operations.GetAccessRequestMaxRisk](../../models/operations/getaccessrequestmaxrisk.md)             | :heavy_check_mark:                                                                                   | How risky an operation is (declared by the plugin metadata).                                         |
| `debugGrant`                                                                                         | [models.AccessRequestDebugGrant](../../models/accessrequestdebuggrant.md)                            | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `status`                                                                                             | [models.AccessRequestStatus](../../models/accessrequeststatus.md)                                    | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `approvedUntil`                                                                                      | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `createdAt`                                                                                          | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `queuedBy`                                                                                           | *string*                                                                                             | :heavy_check_mark:                                                                                   | Who passed the engineer gate; the requester for a plan-less request.                                 |
| `queuedAt`                                                                                           | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `approvedBy`                                                                                         | [operations.GetAccessRequestApprovedBy](../../models/operations/getaccessrequestapprovedby.md)       | :heavy_check_mark:                                                                                   | How and when the customer gate was passed. Null until approved.                                      |
| `deniedBy`                                                                                           | [operations.GetAccessRequestDeniedBy](../../models/operations/getaccessrequestdeniedby.md)           | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `revokedBy`                                                                                          | [operations.GetAccessRequestRevokedBy](../../models/operations/getaccessrequestrevokedby.md)         | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
