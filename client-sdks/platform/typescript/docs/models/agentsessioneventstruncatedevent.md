# AgentSessionEventsTruncatedEvent

## Example Usage

```typescript
import { AgentSessionEventsTruncatedEvent } from "@alienplatform/platform-api/models";

let value: AgentSessionEventsTruncatedEvent = {
  seq: 1033.37,
  createdAt: "1730528196150",
  type: "events_truncated",
  payload: {
    limit: 5615.57,
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `seq`                                                                                                  | *number*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `createdAt`                                                                                            | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `type`                                                                                                 | *"events_truncated"*                                                                                   | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `payload`                                                                                              | [models.AgentSessionEventsTruncatedEventPayload](../models/agentsessioneventstruncatedeventpayload.md) | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
