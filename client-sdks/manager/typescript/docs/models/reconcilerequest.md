# ReconcileRequest

## Example Usage

```typescript
import { ReconcileRequest } from "@alienplatform/manager-api/models";

let value: ReconcileRequest = {
  deploymentId: "<id>",
  observedInventoryBatches: [
    {
      backend: "external",
      complete: false,
      controllerPlatform: "test",
      inventoryScope: "<value>",
      observedAt: new Date("2026-03-15T15:54:44.264Z"),
      resources: [
        {
          displayName: "Hunter72",
          health: "unhealthy",
          lifecycle: "creating",
          partial: false,
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
      controllerPlatform: "local",
      data: {
        data: {
          data: {
            enabled: true,
            keyArn: "<value>",
            keySpec: "<value>",
            keyState: "<value>",
            keyUsage: "<value>",
            status: {
              health: "healthy",
              lifecycle: "running",
            },
          },
          provider: "aws-kms",
        },
        resourceType: "key",
      },
      observedAt: new Date("2026-03-29T08:18:22.529Z"),
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