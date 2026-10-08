# RepositoryPrefix1

## Example Usage

```typescript
import { RepositoryPrefix1 } from "@alienplatform/platform-api/models";

let value: RepositoryPrefix1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `secretRef`                                                                  | [models.RepositoryPrefixSecretRef1](../models/repositoryprefixsecretref1.md) | :heavy_check_mark:                                                           | Reference to a Kubernetes Secret                                             |
