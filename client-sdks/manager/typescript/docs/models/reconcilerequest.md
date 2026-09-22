# ReconcileRequest

## Example Usage

```typescript
import { ReconcileRequest } from "@alienplatform/manager-api/models";

let value: ReconcileRequest = {
  deploymentId: "<id>",
  observedInventoryBatches: [
    {
      backend: "local",
      complete: true,
      controllerPlatform: "test",
      inventoryScope: "<value>",
      observedAt: new Date("2024-03-08T01:42:23.482Z"),
      resources: [
        {
          displayName: "Camden_Denesik91",
          health: "unhealthy",
          lifecycle: "scaling",
          partial: true,
          providerKind: "<value>",
          providerStale: true,
          rawIdentity: "<value>",
          resourceTypeHint: "worker",
        },
      ],
      sourceKind: "<value>",
    },
  ],
  resourceHeartbeats: [
    {
      backend: "external",
      controllerPlatform: "azure",
      data: {
        data: {
          functionName: "<value>",
          functionUrlCorsPresent: false,
          layerCount: 847359,
          status: {
            collectionIssues: [],
            health: "degraded",
            lifecycle: "deleted",
            partial: false,
            stale: true,
          },
          triggerCount: 195082,
          backend: "awsLambda",
        },
        resourceType: "worker",
      },
      observedAt: new Date("2026-04-02T11:29:30.200Z"),
      raw: [],
      resourceId: "<id>",
      resourceType: "worker",
    },
  ],
  session: "<value>",
  state: "Kansas",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `capabilities`                                                             | [models.OperatorCapabilityReport](../models/operatorcapabilityreport.md)[] | :heavy_minus_sign:                                                         | N/A                                                                        |
| `deploymentId`                                                             | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `executionClaim`                                                           | [models.ExecutionClaim](../models/executionclaim.md)                       | :heavy_minus_sign:                                                         | N/A                                                                        |
| `observedInventoryBatches`                                                 | [models.ObservedInventoryBatch](../models/observedinventorybatch.md)[]     | :heavy_minus_sign:                                                         | N/A                                                                        |
| `operatorVersion`                                                          | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `resourceHeartbeats`                                                       | [models.ResourceHeartbeat](../models/resourceheartbeat.md)[]               | :heavy_minus_sign:                                                         | N/A                                                                        |
| `session`                                                                  | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `state`                                                                    | *any*                                                                      | :heavy_check_mark:                                                         | N/A                                                                        |
| `suggestedDelayMs`                                                         | *number*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `updateHeartbeat`                                                          | *boolean*                                                                  | :heavy_minus_sign:                                                         | N/A                                                                        |