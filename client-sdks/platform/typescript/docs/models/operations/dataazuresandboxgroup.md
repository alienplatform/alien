# DataAzureSandboxGroup

Azure: the sandbox group's ARM state. The data plane has no list operation, so a session count
is not available here.

## Example Usage

```typescript
import { DataAzureSandboxGroup } from "@alienplatform/platform-api/models/operations";

let value: DataAzureSandboxGroup = {
  sandboxGroup: "<value>",
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "stopped",
    partial: true,
    stale: false,
  },
  backend: "azureSandboxGroup",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `provisioningState`                                                | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `sandboxGroup`                                                     | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `status`                                                           | [operations.DataStatus74](../../models/operations/datastatus74.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `backend`                                                          | *"azureSandboxGroup"*                                              | :heavy_check_mark:                                                 | N/A                                                                |