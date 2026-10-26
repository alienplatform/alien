# ExternalBindingUsername6

## Example Usage

```typescript
import { ExternalBindingUsername6 } from "@alienplatform/platform-api/models";

let value: ExternalBindingUsername6 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `secretRef`                                                  | [models.UsernameSecretRef6](../models/usernamesecretref6.md) | :heavy_check_mark:                                           | Reference to a Kubernetes Secret                             |