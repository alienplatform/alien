# AgentSyncWireRequest

Inbound sync payload that adds optional receipts without expanding the
public [`AgentSyncRequest`] struct literal.

## Example Usage

```typescript
import { AgentSyncWireRequest } from "@alienplatform/manager-api/models";

let value: AgentSyncWireRequest = {
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
};
```

## Fields

| Field                                                                                                                                                                   | Type                                                                                                                                                                    | Required                                                                                                                                                                | Description                                                                                                                                                             |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `capabilities`                                                                                                                                                          | [models.OperatorCapabilityReport](../models/operatorcapabilityreport.md)[]                                                                                              | :heavy_minus_sign:                                                                                                                                                      | N/A                                                                                                                                                                     |
| `currentState`                                                                                                                                                          | *any*                                                                                                                                                                   | :heavy_minus_sign:                                                                                                                                                      | Current deployment state as reported by the agent.<br/>When present, the manager updates the deployment record to reflect<br/>the agent's progress (status, stack_state, etc.). |
| `deploymentId`                                                                                                                                                          | *string*                                                                                                                                                                | :heavy_check_mark:                                                                                                                                                      | N/A                                                                                                                                                                     |
| `executionClaim`                                                                                                                                                        | [models.ExecutionClaim](../models/executionclaim.md)                                                                                                                    | :heavy_minus_sign:                                                                                                                                                      | N/A                                                                                                                                                                     |
| `observedInventoryBatches`                                                                                                                                              | [models.ObservedInventoryBatch](../models/observedinventorybatch.md)[]                                                                                                  | :heavy_minus_sign:                                                                                                                                                      | N/A                                                                                                                                                                     |
| `operationsReport`                                                                                                                                                      | [models.OperationsReport](../models/operationsreport.md)                                                                                                                | :heavy_minus_sign:                                                                                                                                                      | N/A                                                                                                                                                                     |
| `operatorVersion`                                                                                                                                                       | *string*                                                                                                                                                                | :heavy_minus_sign:                                                                                                                                                      | N/A                                                                                                                                                                     |
| `resourceHeartbeats`                                                                                                                                                    | [models.ResourceHeartbeat](../models/resourceheartbeat.md)[]                                                                                                            | :heavy_minus_sign:                                                                                                                                                      | Managed resource status samples emitted by pull-mode deployment steps.                                                                                                  |
| `session`                                                                                                                                                               | *string*                                                                                                                                                                | :heavy_minus_sign:                                                                                                                                                      | N/A                                                                                                                                                                     |
| `supportsExecutionClaims`                                                                                                                                               | *boolean*                                                                                                                                                               | :heavy_minus_sign:                                                                                                                                                      | N/A                                                                                                                                                                     |
| `operatorImage`                                                                                                                                                         | [models.OperatorImageReport](../models/operatorimagereport.md)                                                                                                          | :heavy_minus_sign:                                                                                                                                                      | N/A                                                                                                                                                                     |