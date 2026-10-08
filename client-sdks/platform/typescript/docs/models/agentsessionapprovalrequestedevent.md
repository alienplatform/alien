# AgentSessionApprovalRequestedEvent

## Example Usage

```typescript
import { AgentSessionApprovalRequestedEvent } from "@alienplatform/platform-api/models";

let value: AgentSessionApprovalRequestedEvent = {
  seq: 8489.57,
  createdAt: "1727488365811",
  type: "approval_requested",
  payload: {
    approvalId: "<id>",
    toolCallId: "<id>",
    toolName: "<value>",
  },
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `seq`                                                                                                      | *number*                                                                                                   | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `createdAt`                                                                                                | *string*                                                                                                   | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `type`                                                                                                     | *"approval_requested"*                                                                                     | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `payload`                                                                                                  | [models.AgentSessionApprovalRequestedEventPayload](../models/agentsessionapprovalrequestedeventpayload.md) | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
