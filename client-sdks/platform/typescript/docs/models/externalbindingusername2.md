# ExternalBindingUsername2

## Example Usage

```typescript
import { ExternalBindingUsername2 } from "@alienplatform/platform-api/models";

let value: ExternalBindingUsername2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `secretRef`                                                  | [models.UsernameSecretRef2](../models/usernamesecretref2.md) | :heavy_check_mark:                                           | Reference to a Kubernetes Secret                             |
