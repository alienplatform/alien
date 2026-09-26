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
  title: "<value>",
  reason: "<value>",
  commands: [],
  operationPattern: "<value>",
  maxRisk: "read-only",
  debugGrant: {
    tool: "kubectl",
    namespace: "braintrust",
    cloudScope: "123456789012/prod-readonly",
  },
  status: "pending-approval",
  approvedUntil: "<value>",
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
| `title`                                                                                                | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `reason`                                                                                               | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `commands`                                                                                             | [operations.GetLiveDebugGrantCommand](../../models/operations/getlivedebuggrantcommand.md)[]           | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `operationPattern`                                                                                     | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `maxRisk`                                                                                              | [operations.GetLiveDebugGrantMaxRisk](../../models/operations/getlivedebuggrantmaxrisk.md)             | :heavy_check_mark:                                                                                     | How risky an operation is (declared by the plugin metadata).                                           |
| `debugGrant`                                                                                           | [models.AccessRequestDebugGrant](../../models/accessrequestdebuggrant.md)                              | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `status`                                                                                               | [models.AccessRequestStatus](../../models/accessrequeststatus.md)                                      | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `approvedUntil`                                                                                        | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |