# SandboxRepositoryPrefix

## Example Usage

```typescript
import { SandboxRepositoryPrefix } from "@alienplatform/platform-api/models";

let value: SandboxRepositoryPrefix = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `secretRef`                                                                              | [models.SandboxRepositoryPrefixSecretRef](../models/sandboxrepositoryprefixsecretref.md) | :heavy_check_mark:                                                                       | Reference to a Kubernetes Secret                                                         |