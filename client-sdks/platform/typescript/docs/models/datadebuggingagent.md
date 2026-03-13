# DataDebuggingAgent

## Example Usage

```typescript
import { DataDebuggingAgent } from "@aliendotdev/platform-api/models";

let value: DataDebuggingAgent = {
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