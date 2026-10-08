# AgentSessionToolCallEvent

## Example Usage

```typescript
import { AgentSessionToolCallEvent } from "@alienplatform/platform-api/models";

let value: AgentSessionToolCallEvent = {
  seq: 400.77,
  createdAt: "1718464824179",
  type: "tool_call",
  payload: {
    toolCallId: "<id>",
    toolName: "<value>",
  },
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `seq`                                                                                    | *number*                                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `createdAt`                                                                              | *string*                                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `type`                                                                                   | *"tool_call"*                                                                            | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `payload`                                                                                | [models.AgentSessionToolCallEventPayload](../models/agentsessiontoolcalleventpayload.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |
