# SyncReleaseRequest

## Example Usage

```typescript
import { SyncReleaseRequest } from "@alienplatform/platform-api/models/operations";

let value: SyncReleaseRequest = {
  workspace: "my-workspace",
  syncReleaseRequest: {
    deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
    session: "<value>",
  },
};
```

## Fields

| Field                                                                                                                                                                                  | Type                                                                                                                                                                                   | Required                                                                                                                                                                               | Description                                                                                                                                                                            | Example                                                                                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `workspace`                                                                                                                                                                            | *string*                                                                                                                                                                               | :heavy_minus_sign:                                                                                                                                                                     | Workspace name. Required for user/session/OAuth requests. Optional for API keys because API keys are workspace-scoped; if provided with an API key, it must match the key's workspace. | my-workspace                                                                                                                                                                           |
| `syncReleaseRequest`                                                                                                                                                                   | [models.SyncReleaseRequest](../../models/syncreleaserequest.md)                                                                                                                        | :heavy_check_mark:                                                                                                                                                                     | N/A                                                                                                                                                                                    |                                                                                                                                                                                        |