# VaultPrefix2

## Example Usage

```typescript
import { VaultPrefix2 } from "@alienplatform/platform-api/models";

let value: VaultPrefix2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `secretRef`                                                        | [models.VaultPrefixSecretRef2](../models/vaultprefixsecretref2.md) | :heavy_check_mark:                                                 | Reference to a Kubernetes Secret                                   |
