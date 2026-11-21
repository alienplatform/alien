# RepositoryPrefix

## Example Usage

```typescript
import { RepositoryPrefix } from "@alienplatform/platform-api/models";

let value: RepositoryPrefix = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `secretRef`                                                                | [models.RepositoryPrefixSecretRef](../models/repositoryprefixsecretref.md) | :heavy_check_mark:                                                         | Reference to a Kubernetes Secret                                           |