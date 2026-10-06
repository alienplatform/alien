# AccessRequestActivityOperation

## Example Usage

```typescript
import { AccessRequestActivityOperation } from "@alienplatform/platform-api/models";

let value: AccessRequestActivityOperation = {
  commandId: "<id>",
  invocationId: "<id>",
  command: "<value>",
  state: "Arizona",
  createdAt: "1707508523705",
  dispatchedAt: "<value>",
  completedAt: "<value>",
  verification: {
    state: "failed",
    reason: null,
  },
};
```

## Fields

| Field                                                                                   | Type                                                                                    | Required                                                                                | Description                                                                             |
| --------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| `commandId`                                                                             | *string*                                                                                | :heavy_check_mark:                                                                      | N/A                                                                                     |
| `invocationId`                                                                          | *string*                                                                                | :heavy_check_mark:                                                                      | The MCP invocation that dispatched the command, when MCP did.                           |
| `command`                                                                               | *string*                                                                                | :heavy_check_mark:                                                                      | `plugin/operation`.                                                                     |
| `state`                                                                                 | *string*                                                                                | :heavy_check_mark:                                                                      | N/A                                                                                     |
| `createdAt`                                                                             | *string*                                                                                | :heavy_check_mark:                                                                      | N/A                                                                                     |
| `dispatchedAt`                                                                          | *string*                                                                                | :heavy_check_mark:                                                                      | N/A                                                                                     |
| `completedAt`                                                                           | *string*                                                                                | :heavy_check_mark:                                                                      | N/A                                                                                     |
| `verification`                                                                          | [models.CommandVerification](../models/commandverification.md)                          | :heavy_check_mark:                                                                      | Verification outcome of an operation command; null for commands that are not operations |