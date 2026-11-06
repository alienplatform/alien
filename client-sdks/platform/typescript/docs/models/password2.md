# Password2

## Example Usage

```typescript
import { Password2 } from "@alienplatform/platform-api/models";

let value: Password2 = {
  secretKeyRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                            | Type                                             | Required                                         | Description                                      |
| ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ |
| `secretKeyRef`                                   | [models.SecretKeyRef](../models/secretkeyref.md) | :heavy_check_mark:                               | Reference to a Kubernetes Secret                 |