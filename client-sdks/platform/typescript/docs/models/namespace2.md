# Namespace2

## Example Usage

```typescript
import { Namespace2 } from "@alienplatform/platform-api/models";

let value: Namespace2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `secretRef`                                                    | [models.NamespaceSecretRef2](../models/namespacesecretref2.md) | :heavy_check_mark:                                             | Reference to a Kubernetes Secret                               |