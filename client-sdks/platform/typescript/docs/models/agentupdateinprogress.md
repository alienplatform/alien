# AgentUpdateInProgress

## Example Usage

```typescript
import { AgentUpdateInProgress } from "@alienplatform/platform-api/models";

let value: AgentUpdateInProgress = {
  state: "inProgress",
  targetVersion: "<value>",
  attempt: 88597,
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `state`                                                | [models.StateInProgress](../models/stateinprogress.md) | :heavy_check_mark:                                     | N/A                                                    |
| `targetVersion`                                        | *string*                                               | :heavy_check_mark:                                     | N/A                                                    |
| `attempt`                                              | *number*                                               | :heavy_check_mark:                                     | N/A                                                    |