# RegistryUrl

## Example Usage

```typescript
import { RegistryUrl } from "@alienplatform/platform-api/models";

let value: RegistryUrl = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `secretRef`                                                      | [models.RegistryUrlSecretRef](../models/registryurlsecretref.md) | :heavy_check_mark:                                               | Reference to a Kubernetes Secret                                 |