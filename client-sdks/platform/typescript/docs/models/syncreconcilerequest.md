# SyncReconcileRequest

Request to reconcile deployment state

## Example Usage

```typescript
import { SyncReconcileRequest } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequest = {
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  state: {
    platform: "azure",
    status: "updating",
  },
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    | Example                                                                        |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `deploymentId`                                                                 | *string*                                                                       | :heavy_check_mark:                                                             | Deployment ID to reconcile state for                                           | dep_0c29fq4a2yjb7kx3smwdgxlc                                                   |
| `session`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | Lock session (push model only) - verifies lock ownership                       |                                                                                |
| `state`                                                                        | [models.SyncReconcileRequestState](../models/syncreconcilerequeststate.md)     | :heavy_check_mark:                                                             | Complete deployment state after step() execution                               |                                                                                |
| `error`                                                                        | [models.SyncReconcileRequestError](../models/syncreconcilerequesterror.md)     | :heavy_minus_sign:                                                             | Deployment error from step() result. Set when deployment fails, null to clear. |                                                                                |
| `updateHeartbeat`                                                              | *boolean*                                                                      | :heavy_minus_sign:                                                             | Update heartbeat timestamp (for successful health checks)                      |                                                                                |