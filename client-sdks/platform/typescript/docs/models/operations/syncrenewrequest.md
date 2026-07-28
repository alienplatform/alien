# SyncRenewRequest

## Example Usage

```typescript
import { SyncRenewRequest } from "@alienplatform/platform-api/models/operations";

let value: SyncRenewRequest = {
  workspace: "my-workspace",
  syncRenewRequest: {
    deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
    deploymentIds: [
      "dep_0c29fq4a2yjb7kx3smwdgxlc",
    ],
    session: "<value>",
  },
};
```

## Fields

| Field                                                                                                                                                                                  | Type                                                                                                                                                                                   | Required                                                                                                                                                                               | Description                                                                                                                                                                            | Example                                                                                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `workspace`                                                                                                                                                                            | *string*                                                                                                                                                                               | :heavy_minus_sign:                                                                                                                                                                     | Workspace name. Required for user/session/OAuth requests. Optional for API keys because API keys are workspace-scoped; if provided with an API key, it must match the key's workspace. | my-workspace                                                                                                                                                                           |
| `syncRenewRequest`                                                                                                                                                                     | [models.SyncRenewRequest](../../models/syncrenewrequest.md)                                                                                                                            | :heavy_check_mark:                                                                                                                                                                     | N/A                                                                                                                                                                                    |                                                                                                                                                                                        |
