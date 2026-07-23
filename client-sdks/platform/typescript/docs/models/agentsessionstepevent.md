# AgentSessionStepEvent

## Example Usage

```typescript
import { AgentSessionStepEvent } from "@alienplatform/platform-api/models";

let value: AgentSessionStepEvent = {
  seq: 1186.78,
  createdAt: "1731419010075",
  type: "step",
  payload: {
    stepId: "<id>",
    title: "<value>",
    status: "in_progress",
  },
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `seq`                                                                            | *number*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `createdAt`                                                                      | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `type`                                                                           | *"step"*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `payload`                                                                        | [models.AgentSessionStepEventPayload](../models/agentsessionstepeventpayload.md) | :heavy_check_mark:                                                               | N/A                                                                              |
