# ListAccessRequestsItem

## Example Usage

```typescript
import { ListAccessRequestsItem } from "@alienplatform/platform-api/models/operations";

let value: ListAccessRequestsItem = {
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
  reason: null,
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
  maxRisk: null,
  status: "queued",
  approvedUntil: "<value>",
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `id`                                                                                               | *string*                                                                                           | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `deploymentId`                                                                                     | *string*                                                                                           | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `deployment`                                                                                       | [operations.ListAccessRequestsDeployment](../../models/operations/listaccessrequestsdeployment.md) | :heavy_minus_sign:                                                                                 | N/A                                                                                                |
| `remediationPlanId`                                                                                | *string*                                                                                           | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `title`                                                                                            | *string*                                                                                           | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `reason`                                                                                           | *string*                                                                                           | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `commands`                                                                                         | [operations.ListAccessRequestsCommand](../../models/operations/listaccessrequestscommand.md)[]     | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `operationPattern`                                                                                 | *string*                                                                                           | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `maxRisk`                                                                                          | [operations.ListAccessRequestsMaxRisk](../../models/operations/listaccessrequestsmaxrisk.md)       | :heavy_check_mark:                                                                                 | How risky an operation is (declared by the plugin metadata).                                       |
| `status`                                                                                           | [models.AccessRequestStatus](../../models/accessrequeststatus.md)                                  | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `approvedUntil`                                                                                    | *string*                                                                                           | :heavy_check_mark:                                                                                 | N/A                                                                                                |