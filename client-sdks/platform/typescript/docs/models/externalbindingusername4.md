# ExternalBindingUsername4

## Example Usage

```typescript
import { ExternalBindingUsername4 } from "@alienplatform/platform-api/models";

let value: ExternalBindingUsername4 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `secretRef`                                                  | [models.UsernameSecretRef4](../models/usernamesecretref4.md) | :heavy_check_mark:                                           | Reference to a Kubernetes Secret                             |
