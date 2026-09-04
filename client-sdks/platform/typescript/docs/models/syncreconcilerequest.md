# SyncReconcileRequest

Request to reconcile deployment state

## Example Usage

```typescript
import { SyncReconcileRequest } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequest = {
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  operationId: "duop_0vtxpb1sw4sbcdwg2xo37q6",
  attemptId: "duat_uve04tou5eoua3q17dar1pz",
  state: {
    platform: "azure",
    protocolVersion: 583290,
    status: "delete-failed",
  },
};
```

## Fields

| Field                                                                                     | Type                                                                                      | Required                                                                                  | Description                                                                               | Example                                                                                   |
| ----------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| `deploymentId`                                                                            | *string*                                                                                  | :heavy_check_mark:                                                                        | Deployment ID to reconcile state for                                                      | dep_0c29fq4a2yjb7kx3smwdgxlc                                                              |
| `session`                                                                                 | *string*                                                                                  | :heavy_minus_sign:                                                                        | Lock session (push model only) - verifies lock ownership                                  |                                                                                           |
| `operationId`                                                                             | *string*                                                                                  | :heavy_minus_sign:                                                                        | Immutable operation claimed at acquisition. Required with attemptId for update execution. | duop_0vtxpb1sw4sbcdwg2xo37q6                                                              |
| `attemptId`                                                                               | *string*                                                                                  | :heavy_minus_sign:                                                                        | Execution attempt claimed at acquisition. Required with operationId for update execution. | duat_uve04tou5eoua3q17dar1pz                                                              |
| `state`                                                                                   | [models.DeploymentState](../models/deploymentstate.md)                                    | :heavy_check_mark:                                                                        | N/A                                                                                       |                                                                                           |
| `updateHeartbeat`                                                                         | *boolean*                                                                                 | :heavy_minus_sign:                                                                        | Update heartbeat timestamp (for successful health checks)                                 |                                                                                           |
| `suggestedDelayMs`                                                                        | *number*                                                                                  | :heavy_minus_sign:                                                                        | Delay before this deployment should be acquired again.                                    |                                                                                           |
| `resourceHeartbeats`                                                                      | [models.ResourceHeartbeat](../models/resourceheartbeat.md)[]                              | :heavy_minus_sign:                                                                        | Latest typed resource heartbeats collected during this step.                              |                                                                                           |
| `observedInventoryBatches`                                                                | [models.ObservedInventoryBatch](../models/observedinventorybatch.md)[]                    | :heavy_minus_sign:                                                                        | Observed raw-resource inventory batches read during this step.                            |                                                                                           |
| `capabilities`                                                                            | [models.OperatorCapabilityReport](../models/operatorcapabilityreport.md)[]                | :heavy_minus_sign:                                                                        | Operator-reported runtime capabilities.                                                   |                                                                                           |
| `operatorVersion`                                                                         | *string*                                                                                  | :heavy_minus_sign:                                                                        | Operator binary version reported by the runtime.                                          |                                                                                           |