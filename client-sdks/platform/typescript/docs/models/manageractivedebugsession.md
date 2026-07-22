# ManagerActiveDebugSession

## Example Usage

```typescript
import { ManagerActiveDebugSession } from "@alienplatform/platform-api/models";

let value: ManagerActiveDebugSession = {
  id: "dbg_HOXmkmT9UPYlsnxqSNlEGoXL",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  state: "stopped",
  expiresAt: new Date("2025-04-05T19:55:04.463Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the debug session.                                                      | dbg_HOXmkmT9UPYlsnxqSNlEGoXL                                                                  |
| `deploymentId`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the deployment.                                                         | dep_0c29fq4a2yjb7kx3smwdgxlc                                                                  |
| `state`                                                                                       | [models.DebugSessionState](../models/debugsessionstate.md)                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `expiresAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `backendTargetId`                                                                             | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |
