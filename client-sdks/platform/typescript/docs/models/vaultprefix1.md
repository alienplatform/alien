# VaultPrefix1

## Example Usage

```typescript
import { VaultPrefix1 } from "@alienplatform/platform-api/models";

let value: VaultPrefix1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `secretRef`                                                        | [models.VaultPrefixSecretRef1](../models/vaultprefixsecretref1.md) | :heavy_check_mark:                                                 | Reference to a Kubernetes Secret                                   |