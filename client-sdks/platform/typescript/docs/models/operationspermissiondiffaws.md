# OperationsPermissionDiffAws

## Example Usage

```typescript
import { OperationsPermissionDiffAws } from "@alienplatform/platform-api/models";

let value: OperationsPermissionDiffAws = {
  added: [
    {
      effect: "Deny",
      actions: [
        "<value 1>",
        "<value 2>",
        "<value 3>",
      ],
      resources: [],
      condition: {
        "key": {
          "key": "<value>",
          "key1": "<value>",
        },
        "key1": {
          "key": "<value>",
        },
        "key2": {
          "key": "<value>",
        },
      },
      sources: [],
    },
  ],
  removed: [
    {
      effect: "Allow",
      actions: [
        "<value 1>",
        "<value 2>",
      ],
      resources: [
        "<value 1>",
      ],
      condition: {},
      sources: [],
    },
  ],
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `added`                                                                                            | [models.OperationsAwsPermissionDiffStatement](../models/operationsawspermissiondiffstatement.md)[] | :heavy_check_mark:                                                                                 | IAM statements this change adds.                                                                   |
| `removed`                                                                                          | [models.OperationsAwsPermissionDiffStatement](../models/operationsawspermissiondiffstatement.md)[] | :heavy_check_mark:                                                                                 | IAM statements this change removes from the generated cloud configuration.                         |