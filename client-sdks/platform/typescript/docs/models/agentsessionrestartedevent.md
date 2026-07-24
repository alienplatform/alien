# AgentSessionRestartedEvent

## Example Usage

```typescript
import { AgentSessionRestartedEvent } from "@alienplatform/platform-api/models";

let value: AgentSessionRestartedEvent = {
  seq: 9503.12,
  createdAt: "1734099609945",
  type: "session_restarted",
  payload: {
    reason: "<value>",
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `seq`                                                                                      | *number*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `createdAt`                                                                                | *string*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `type`                                                                                     | *"session_restarted"*                                                                      | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `payload`                                                                                  | [models.AgentSessionRestartedEventPayload](../models/agentsessionrestartedeventpayload.md) | :heavy_check_mark:                                                                         | N/A                                                                                        |
