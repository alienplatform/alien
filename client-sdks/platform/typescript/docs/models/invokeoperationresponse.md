# InvokeOperationResponse

## Example Usage

```typescript
import { InvokeOperationResponse } from "@alienplatform/platform-api/models";

let value: InvokeOperationResponse = {
  plugin: "<value>",
  operation: "<value>",
  tier: "read-only",
  decision: "manual",
  status: "pending-approval",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `plugin`                                                                               | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `operation`                                                                            | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `tier`                                                                                 | [models.InvokeOperationResponseTier](../models/invokeoperationresponsetier.md)         | :heavy_check_mark:                                                                     | The operation's declared risk tier (informational).                                    |
| `decision`                                                                             | [models.InvokeOperationResponseDecision](../models/invokeoperationresponsedecision.md) | :heavy_check_mark:                                                                     | The approval decision the policy resolved to.                                          |
| `status`                                                                               | [models.InvokeOperationResponseStatus](../models/invokeoperationresponsestatus.md)     | :heavy_check_mark:                                                                     | dispatched: auto-approved and running. pending-approval: needs customer sign-off.      |
| `commandId`                                                                            | *string*                                                                               | :heavy_minus_sign:                                                                     | The created operation-command id when dispatched (absent when pending).                |