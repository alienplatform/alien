# AgentSessionStepEventPayload

## Example Usage

```typescript
import { AgentSessionStepEventPayload } from "@alienplatform/platform-api/models";

let value: AgentSessionStepEventPayload = {
  stepId: "<id>",
  title: "<value>",
  status: "in_progress",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `stepId`                                                                       | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `title`                                                                        | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `status`                                                                       | [models.AgentSessionStepEventStatus](../models/agentsessionstepeventstatus.md) | :heavy_check_mark:                                                             | N/A                                                                            |
