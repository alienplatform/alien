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
          name: "<value>",
          privateEndpointConnectionCount: 289000,
          status: {
            collectionIssues: [],
            health: "healthy",
            lifecycle: "running",
            partial: false,
            stale: true,
          },
        },
        resourceType: "azure_service_bus_namespace",
      },
      observedAt: new Date("2026-04-15T12:52:58.852Z"),
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
| `observedInventoryBatches`                                                 | [models.ObservedInventoryBatch](../models/observedinventorybatch.md)[]     | :heavy_minus_sign:                                                         | N/A                                                                        |
| `operatorVersion`                                                          | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `resourceHeartbeats`                                                       | [models.ResourceHeartbeat](../models/resourceheartbeat.md)[]               | :heavy_minus_sign:                                                         | N/A                                                                        |
| `session`                                                                  | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `state`                                                                    | *any*                                                                      | :heavy_check_mark:                                                         | N/A                                                                        |
| `suggestedDelayMs`                                                         | *number*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `updateHeartbeat`                                                          | *boolean*                                                                  | :heavy_minus_sign:                                                         | N/A                                                                        |
