# ExternalBindingUsername1

## Example Usage

```typescript
import { ExternalBindingUsername1 } from "@alienplatform/platform-api/models";

let value: ExternalBindingUsername1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `secretRef`                                                  | [models.UsernameSecretRef1](../models/usernamesecretref1.md) | :heavy_check_mark:                                           | Reference to a Kubernetes Secret                             |