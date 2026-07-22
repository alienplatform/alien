# AgentSessionStatusEvent

## Example Usage

```typescript
import { AgentSessionStatusEvent } from "@alienplatform/platform-api/models";

let value: AgentSessionStatusEvent = {
  seq: 4210.33,
  createdAt: "1710340330705",
  type: "status",
  payload: {
    status: "<value>",
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `seq`                                                                                | *number*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `createdAt`                                                                          | *string*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `type`                                                                               | *"status"*                                                                           | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `payload`                                                                            | [models.AgentSessionStatusEventPayload](../models/agentsessionstatuseventpayload.md) | :heavy_check_mark:                                                                   | N/A                                                                                  |
