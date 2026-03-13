# DataStackStep

## Example Usage

```typescript
import { DataStackStep } from "@alienplatform/platform-api/models";

let value: DataStackStep = {
  nextState: {
    platform: "test",
    resourcePrefix: "<value>",
    resources: {
      "key": {
        config: {
          id: "<id>",
          type: "<value>",
        },
        status: "running",
        type: "<value>",
      },
    },
  },
  type: "StackStep",
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `nextState`                                                                                          | [models.NextState](../models/nextstate.md)                                                           | :heavy_check_mark:                                                                                   | Represents the collective state of all resources in a stack, including platform and pending actions. |
| `suggestedDelayMs`                                                                                   | *number*                                                                                             | :heavy_minus_sign:                                                                                   | An suggested duration to wait before executing the next step.                                        |
| `type`                                                                                               | *"StackStep"*                                                                                        | :heavy_check_mark:                                                                                   | N/A                                                                                                  |