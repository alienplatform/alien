# Rule

## Example Usage

```typescript
import { Rule } from "@alienplatform/platform-api/models";

let value: Rule = {
  apiGroup: "<value>",
  resource: "<value>",
  verbs: [],
  reason: "<value>",
};
```

## Fields

| Field                              | Type                               | Required                           | Description                        |
| ---------------------------------- | ---------------------------------- | ---------------------------------- | ---------------------------------- |
| `apiGroup`                         | *string*                           | :heavy_check_mark:                 | N/A                                |
| `resource`                         | *string*                           | :heavy_check_mark:                 | N/A                                |
| `verbs`                            | [models.Verb](../models/verb.md)[] | :heavy_check_mark:                 | N/A                                |
| `resourceNames`                    | *string*[]                         | :heavy_minus_sign:                 | N/A                                |
| `reason`                           | *string*                           | :heavy_check_mark:                 | N/A                                |