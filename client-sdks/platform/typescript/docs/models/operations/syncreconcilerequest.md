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

| Field                                                                                                                                                                                  | Type                                                                                                                                                                                   | Required                                                                                                                                                                               | Description                                                                                                                                                                            | Example                                                                                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `workspace`                                                                                                                                                                            | *string*                                                                                                                                                                               | :heavy_minus_sign:                                                                                                                                                                     | Workspace name. Required for user/session/OAuth requests. Optional for API keys because API keys are workspace-scoped; if provided with an API key, it must match the key's workspace. | my-workspace                                                                                                                                                                           |
| `syncReconcileRequest`                                                                                                                                                                 | [models.SyncReconcileRequest](../../models/syncreconcilerequest.md)                                                                                                                    | :heavy_check_mark:                                                                                                                                                                     | N/A                                                                                                                                                                                    |                                                                                                                                                                                        |