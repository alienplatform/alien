# PasswordSecretArn

## Example Usage

```typescript
import { PasswordSecretArn } from "@alienplatform/platform-api/models";

let value: PasswordSecretArn = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `secretRef`                                                                  | [models.PasswordSecretArnSecretRef](../models/passwordsecretarnsecretref.md) | :heavy_check_mark:                                                           | Reference to a Kubernetes Secret                                             |
