# PasswordSecretUri

## Example Usage

```typescript
import { PasswordSecretUri } from "@alienplatform/platform-api/models";

let value: PasswordSecretUri = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `secretRef`                                                                  | [models.PasswordSecretUriSecretRef](../models/passwordsecreturisecretref.md) | :heavy_check_mark:                                                           | Reference to a Kubernetes Secret                                             |
