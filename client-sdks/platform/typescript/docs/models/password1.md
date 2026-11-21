# Password1

## Example Usage

```typescript
import { Password1 } from "@alienplatform/platform-api/models";

let value: Password1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `secretRef`                                                | [models.PasswordSecretRef](../models/passwordsecretref.md) | :heavy_check_mark:                                         | Reference to a Kubernetes Secret                           |