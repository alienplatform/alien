# SyncReconcileRequest

Request to reconcile deployment state

## Example Usage

```typescript
import { SyncReconcileRequest } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequest = {
  deploymentId: "ag_pnj2da55wi5sxbdcav9t273je",
  state: {
    platform: "azure",
    status: "updating",
  },
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    | Example                                                                        |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `deploymentId`                                                                 | *string*                                                                       | :heavy_check_mark:                                                             | Deployment ID to reconcile state for                                           | ag_pnj2da55wi5sxbdcav9t273je                                                   |
| `session`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | Lock session (push model only) - verifies lock ownership                       |                                                                                |
| `state`                                                                        | [models.SyncReconcileRequestState](../models/syncreconcilerequeststate.md)     | :heavy_check_mark:                                                             | Complete deployment state after step() execution                               |                                                                                |
| `error`                                                                        | [models.SyncReconcileRequestError](../models/syncreconcilerequesterror.md)     | :heavy_minus_sign:                                                             | Deployment error from step() result. Set when deployment fails, null to clear. |                                                                                |
| `updateHeartbeat`                                                              | *boolean*                                                                      | :heavy_minus_sign:                                                             | Update heartbeat timestamp (for successful health checks)                      |                                                                                |