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
          status: {
            collectionIssues: [],
            health: "healthy",
            lifecycle: "scaling",
            partial: true,
            stale: false,
          },
          workloadProfileCount: 324464,
          workloadProfiles: [
            {
              name: "<value>",
              workloadProfileType: "<value>",
            },
          ],
        },
        resourceType: "azure_container_apps_environment",
      },
      observedAt: new Date("2024-12-01T23:55:46.090Z"),
      raw: [
        {
          body: "<value>",
          collectedAt: new Date("2026-05-11T10:48:38.268Z"),
          format: "text",
          source: "<value>",
          truncated: false,
        },
      ],
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