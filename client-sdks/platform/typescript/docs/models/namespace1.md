# Namespace1

## Example Usage

```typescript
import { Namespace1 } from "@alienplatform/platform-api/models";

let value: Namespace1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `secretRef`                                                    | [models.NamespaceSecretRef1](../models/namespacesecretref1.md) | :heavy_check_mark:                                             | Reference to a Kubernetes Secret                               |
