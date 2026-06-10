# SyncReconcileRequest

## Example Usage

```typescript
import { SyncReconcileRequest } from "@alienplatform/platform-api/models/operations";

let value: SyncReconcileRequest = {
  workspace: "my-workspace",
  syncReconcileRequest: {
    deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
    state: {
      platform: "azure",
      protocolVersion: 583290,
      status: "delete-failed",
    },
  },
};
```

## Fields

| Field                                                                                                                                                                     | Type                                                                                                                                                                      | Required                                                                                                                                                                  | Description                                                                                                                                                               | Example                                                                                                                                                                   |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `workspace`                                                                                                                                                               | *string*                                                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                        | Workspace name. Defaults to your last workspace (user auth) or your API key's workspace (token auth). When using an API key, if provided, must match the key's workspace. | my-workspace                                                                                                                                                              |
| `syncReconcileRequest`                                                                                                                                                    | [models.SyncReconcileRequest](../../models/syncreconcilerequest.md)                                                                                                       | :heavy_check_mark:                                                                                                                                                        | N/A                                                                                                                                                                       |                                                                                                                                                                           |