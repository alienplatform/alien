# RepositoryPrefix2

## Example Usage

```typescript
import { RepositoryPrefix2 } from "@alienplatform/platform-api/models";

let value: RepositoryPrefix2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `secretRef`                                                                  | [models.RepositoryPrefixSecretRef2](../models/repositoryprefixsecretref2.md) | :heavy_check_mark:                                                           | Reference to a Kubernetes Secret                                             |
