# ExternalBindingUsername3

## Example Usage

```typescript
import { ExternalBindingUsername3 } from "@alienplatform/platform-api/models";

let value: ExternalBindingUsername3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `secretRef`                                                  | [models.UsernameSecretRef3](../models/usernamesecretref3.md) | :heavy_check_mark:                                           | Reference to a Kubernetes Secret                             |