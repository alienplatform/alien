# SandboxHeartbeatDataAzureSandboxGroup

Azure: the sandbox group's ARM state. The data plane has no list operation, so a session count
is not available here.

## Example Usage

```typescript
import { SandboxHeartbeatDataAzureSandboxGroup } from "@alienplatform/manager-api/models";

let value: SandboxHeartbeatDataAzureSandboxGroup = {
  sandboxGroup: "<value>",
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "unknown",
    partial: true,
    stale: false,
  },
  backend: "azureSandboxGroup",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `provisioningState`                                                  | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `sandboxGroup`                                                       | *string*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `status`                                                             | [models.SandboxHeartbeatStatus](../models/sandboxheartbeatstatus.md) | :heavy_check_mark:                                                   | N/A                                                                  |
| `backend`                                                            | *"azureSandboxGroup"*                                                | :heavy_check_mark:                                                   | N/A                                                                  |