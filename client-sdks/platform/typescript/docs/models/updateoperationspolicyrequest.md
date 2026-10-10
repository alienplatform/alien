# UpdateOperationsPolicyRequest

## Example Usage

```typescript
import { UpdateOperationsPolicyRequest } from "@alienplatform/platform-api/models";

let value: UpdateOperationsPolicyRequest = {
  rules: [],
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `rules`                                                            | [models.OperationsPolicyRule](../models/operationspolicyrule.md)[] | :heavy_check_mark:                                                 | The full rule set (replaces the previous one).                     |