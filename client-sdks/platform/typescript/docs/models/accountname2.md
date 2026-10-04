# AccountName2

## Example Usage

```typescript
import { AccountName2 } from "@alienplatform/platform-api/models";

let value: AccountName2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `secretRef`                                                        | [models.AccountNameSecretRef2](../models/accountnamesecretref2.md) | :heavy_check_mark:                                                 | Reference to a Kubernetes Secret                                   |