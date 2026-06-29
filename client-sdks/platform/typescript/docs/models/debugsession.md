# DebugSession

## Example Usage

```typescript
import { DebugSession } from "@alienplatform/platform-api/models";

let value: DebugSession = {
  id: "dbg_HOXmkmT9UPYlsnxqSNlEGoXL",
  state: "running",
  mode: "pull",
  presignedUrls: {
    "key": {
      readUrl: "https://frozen-rim.name/",
      writeUrl: "https://tedious-farmer.org/",
    },
  },
  createdAt: new Date("2024-09-20T12:44:28.084Z"),
  expiresAt: new Date("2024-08-23T07:27:56.959Z"),
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  projectId: "prj_mcytp6z3j91f7tn5ryqsfwtr",
  workspaceId: "ws_It13CUaGEhLLAB87simX0",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the debug session.                                                      | dbg_HOXmkmT9UPYlsnxqSNlEGoXL                                                                  |
| `owner`                                                                                       | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |
| `state`                                                                                       | [models.DebugSessionState](../models/debugsessionstate.md)                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `mode`                                                                                        | [models.DebugSessionMode](../models/debugsessionmode.md)                                      | :heavy_check_mark:                                                                            | Deployment model: how updates are delivered to the remote environment.                        |                                                                                               |
| `provider`                                                                                    | [models.DebugSessionProvider](../models/debugsessionprovider.md)                              | :heavy_minus_sign:                                                                            | Represents the target cloud platform.                                                         |                                                                                               |
| `presignedUrls`                                                                               | Record<string, [models.DebugPackagePresignedURLs](../models/debugpackagepresignedurls.md)>    | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `error`                                                                                       | *any*                                                                                         | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |
| `createdAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `expiresAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `deploymentId`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the deployment.                                                         | dep_0c29fq4a2yjb7kx3smwdgxlc                                                                  |
| `projectId`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the project.                                                            | prj_mcytp6z3j91f7tn5ryqsfwtr                                                                  |
| `workspaceId`                                                                                 | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the workspace.                                                          | ws_It13CUaGEhLLAB87simX0                                                                      |