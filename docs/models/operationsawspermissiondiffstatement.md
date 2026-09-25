# OperationsAwsPermissionDiffStatement

## Example Usage

```typescript
import { OperationsAwsPermissionDiffStatement } from "@alienplatform/platform-api/models";

let value: OperationsAwsPermissionDiffStatement = {
  effect: "Deny",
  actions: [
    "<value 1>",
    "<value 2>",
  ],
  resources: [],
  condition: {
    "key": {
      "key": "<value>",
      "key1": "<value>",
    },
    "key1": {
      "key": "<value>",
      "key1": "<value>",
      "key2": "<value>",
    },
  },
  sources: [
    {
      plugin: "<value>",
      operation: "<value>",
      reason: "<value>",
    },
  ],
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `effect`                                                                                                     | [models.OperationsAwsPermissionDiffStatementEffect](../models/operationsawspermissiondiffstatementeffect.md) | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `actions`                                                                                                    | *string*[]                                                                                                   | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `resources`                                                                                                  | *string*[]                                                                                                   | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `condition`                                                                                                  | Record<string, Record<string, *string*>>                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `sources`                                                                                                    | [models.OperationsPermissionDiffSource](../models/operationspermissiondiffsource.md)[]                       | :heavy_check_mark:                                                                                           | N/A                                                                                                          |