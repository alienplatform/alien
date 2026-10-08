# OperationsPolicyResponse

## Example Usage

```typescript
import { OperationsPolicyResponse } from "@alienplatform/platform-api/models";

let value: OperationsPolicyResponse = {
  rules: [
    {
      pattern: "<value>",
      decision: "auto",
    },
  ],
  default: "manual",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `rules`                                                            | [models.OperationsPolicyRule](../models/operationspolicyrule.md)[] | :heavy_check_mark:                                                 | N/A                                                                |
| `default`                                                          | [models.DefaultEnum](../models/defaultenum.md)                     | :heavy_check_mark:                                                 | Decision for commands no rule matches.                             |