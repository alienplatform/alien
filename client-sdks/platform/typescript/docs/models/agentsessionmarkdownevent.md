# AgentSessionMarkdownEvent

## Example Usage

```typescript
import { AgentSessionMarkdownEvent } from "@alienplatform/platform-api/models";

let value: AgentSessionMarkdownEvent = {
  seq: 4742.82,
  createdAt: "1714119157487",
  type: "markdown",
  payload: {
    text: "<value>",
  },
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `seq`                                                                                    | *number*                                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `createdAt`                                                                              | *string*                                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `type`                                                                                   | *"markdown"*                                                                             | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `payload`                                                                                | [models.AgentSessionMarkdownEventPayload](../models/agentsessionmarkdowneventpayload.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |
