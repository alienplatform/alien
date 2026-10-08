# AccountName1

## Example Usage

```typescript
import { AccountName1 } from "@alienplatform/platform-api/models";

let value: AccountName1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `secretRef`                                                        | [models.AccountNameSecretRef1](../models/accountnamesecretref1.md) | :heavy_check_mark:                                                 | Reference to a Kubernetes Secret                                   |