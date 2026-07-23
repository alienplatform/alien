# EventDataDebuggingAgent

## Example Usage

```typescript
import { EventDataDebuggingAgent } from "@alienplatform/platform-api/models";

let value: EventDataDebuggingAgent = {
  agentId: "<id>",
  debugSessionId: "<id>",
  type: "DebuggingAgent",
};
```

## Fields

| Field                          | Type                           | Required                       | Description                    |
| ------------------------------ | ------------------------------ | ------------------------------ | ------------------------------ |
| `agentId`                      | *string*                       | :heavy_check_mark:             | ID of the agent being debugged |
| `debugSessionId`               | *string*                       | :heavy_check_mark:             | ID of the debug session        |
| `type`                         | *"DebuggingAgent"*             | :heavy_check_mark:             | N/A                            |
