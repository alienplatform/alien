# RegistryName

## Example Usage

```typescript
import { RegistryName } from "@alienplatform/platform-api/models";

let value: RegistryName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `secretRef`                                                        | [models.RegistryNameSecretRef](../models/registrynamesecretref.md) | :heavy_check_mark:                                                 | Reference to a Kubernetes Secret                                   |
