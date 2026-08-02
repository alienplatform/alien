# AgentSyncRequest

## Example Usage

```typescript
import { AgentSyncRequest } from "@alienplatform/manager-api/models";

let value: AgentSyncRequest = {
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
};
```

## Fields

| Field                                                                                                                                                                   | Type                                                                                                                                                                    | Required                                                                                                                                                                | Description                                                                                                                                                             |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `capabilities`                                                                                                                                                          | [models.OperatorCapabilityReport](../models/operatorcapabilityreport.md)[]                                                                                              | :heavy_minus_sign:                                                                                                                                                      | N/A                                                                                                                                                                     |
| `currentState`                                                                                                                                                          | *any*                                                                                                                                                                   | :heavy_minus_sign:                                                                                                                                                      | Current deployment state as reported by the agent.<br/>When present, the manager updates the deployment record to reflect<br/>the agent's progress (status, stack_state, etc.). |
| `deploymentId`                                                                                                                                                          | *string*                                                                                                                                                                | :heavy_check_mark:                                                                                                                                                      | N/A                                                                                                                                                                     |
| `observedInventoryBatches`                                                                                                                                              | [models.ObservedInventoryBatch](../models/observedinventorybatch.md)[]                                                                                                  | :heavy_minus_sign:                                                                                                                                                      | N/A                                                                                                                                                                     |
| `operatorVersion`                                                                                                                                                       | *string*                                                                                                                                                                | :heavy_minus_sign:                                                                                                                                                      | N/A                                                                                                                                                                     |
| `resourceHeartbeats`                                                                                                                                                    | [models.ResourceHeartbeat](../models/resourceheartbeat.md)[]                                                                                                            | :heavy_minus_sign:                                                                                                                                                      | Managed resource status samples emitted by pull-mode deployment steps.                                                                                                  |