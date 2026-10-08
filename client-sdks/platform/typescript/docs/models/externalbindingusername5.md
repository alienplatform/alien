# ExternalBindingUsername5

## Example Usage

```typescript
import { ExternalBindingUsername5 } from "@alienplatform/platform-api/models";

let value: ExternalBindingUsername5 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `secretRef`                                                  | [models.UsernameSecretRef5](../models/usernamesecretref5.md) | :heavy_check_mark:                                           | Reference to a Kubernetes Secret                             |
