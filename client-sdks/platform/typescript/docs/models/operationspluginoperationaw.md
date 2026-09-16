# OperationsPluginOperationAw

## Example Usage

```typescript
import { OperationsPluginOperationAw } from "@alienplatform/platform-api/models";

let value: OperationsPluginOperationAw = {
  effect: "Allow",
  actions: [
    "<value 1>",
  ],
  resources: [
    "<value 1>",
  ],
  condition: {
    "key": {
      "key": "<value>",
      "key1": "<value>",
    },
  },
  reason: "<value>",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `effect`                                                                               | [models.OperationsPluginOperationEffect](../models/operationspluginoperationeffect.md) | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `actions`                                                                              | *string*[]                                                                             | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `resources`                                                                            | *string*[]                                                                             | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `condition`                                                                            | Record<string, Record<string, *string*>>                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `reason`                                                                               | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |