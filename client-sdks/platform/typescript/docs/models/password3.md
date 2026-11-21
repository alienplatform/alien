# Password3

## Example Usage

```typescript
import { Password3 } from "@alienplatform/platform-api/models";

let value: Password3 = {
  secretKeyRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `secretKeyRef`                                     | [models.SecretKeyRef2](../models/secretkeyref2.md) | :heavy_check_mark:                                 | Reference to a Kubernetes Secret                   |