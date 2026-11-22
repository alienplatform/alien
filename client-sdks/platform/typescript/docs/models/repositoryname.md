# RepositoryName

## Example Usage

```typescript
import { RepositoryName } from "@alienplatform/platform-api/models";

let value: RepositoryName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `secretRef`                                                            | [models.RepositoryNameSecretRef](../models/repositorynamesecretref.md) | :heavy_check_mark:                                                     | Reference to a Kubernetes Secret                                       |