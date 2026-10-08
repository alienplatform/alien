# VaultPrefix3

## Example Usage

```typescript
import { VaultPrefix3 } from "@alienplatform/platform-api/models";

let value: VaultPrefix3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `secretRef`                                                        | [models.VaultPrefixSecretRef3](../models/vaultprefixsecretref3.md) | :heavy_check_mark:                                                 | Reference to a Kubernetes Secret                                   |
