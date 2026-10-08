# VaultName

## Example Usage

```typescript
import { VaultName } from "@alienplatform/platform-api/models";

let value: VaultName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `secretRef`                                                  | [models.VaultNameSecretRef](../models/vaultnamesecretref.md) | :heavy_check_mark:                                           | Reference to a Kubernetes Secret                             |