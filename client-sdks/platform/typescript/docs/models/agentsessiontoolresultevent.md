# AgentSessionToolResultEvent

## Example Usage

```typescript
import { AgentSessionToolResultEvent } from "@alienplatform/platform-api/models";

let value: AgentSessionToolResultEvent = {
  seq: 4758.06,
  createdAt: "1735429468512",
  type: "tool_result",
  payload: {
    toolCallId: "<id>",
    toolName: "<value>",
    ok: true,
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `seq`                                                                                        | *number*                                                                                     | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `createdAt`                                                                                  | *string*                                                                                     | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `type`                                                                                       | *"tool_result"*                                                                              | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `payload`                                                                                    | [models.AgentSessionToolResultEventPayload](../models/agentsessiontoolresulteventpayload.md) | :heavy_check_mark:                                                                           | N/A                                                                                          |
